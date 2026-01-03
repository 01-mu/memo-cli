use regex::Regex;
use rusqlite::{params, Connection, OptionalExtension};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const DB_CAP: usize = 200;
const DEFAULT_LIMIT: usize = 10;

fn state_db_path() -> PathBuf {
    let base = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("~/.local/state"));
    let base = if base.to_string_lossy().starts_with("~") {
        let home = env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join(base.to_string_lossy().trim_start_matches("~/"))
    } else {
        base
    };
    let db_path = base.join("memo").join("memo.sqlite3");
    if let Some(parent) = db_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    db_path
}

fn connect_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(state_db_path())?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS memos (\
         id INTEGER PRIMARY KEY AUTOINCREMENT, \
         cmd TEXT NOT NULL, \
         created_at INTEGER NOT NULL)",
        [],
    )?;
    Ok(conn)
}

fn enforce_cap(conn: &Connection) -> rusqlite::Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM memos", [], |row| row.get(0))?;
    if count as usize <= DB_CAP {
        return Ok(());
    }
    let to_delete = count - DB_CAP as i64;
    conn.execute(
        "DELETE FROM memos WHERE id IN (\
         SELECT id FROM memos ORDER BY id ASC LIMIT ?)",
        params![to_delete],
    )?;
    Ok(())
}

fn insert_cmd(conn: &Connection, cmd: &str) -> rusqlite::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    conn.execute(
        "INSERT INTO memos (cmd, created_at) VALUES (?, ?)",
        params![cmd, now],
    )?;
    enforce_cap(conn)?;
    Ok(())
}

fn last_saved_cmd(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT cmd FROM memos ORDER BY id DESC LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional()
}

fn list_cmds(conn: &Connection, limit: usize, query: Option<&str>) -> rusqlite::Result<Vec<(usize, String)>> {
    let mut stmt = conn.prepare("SELECT cmd FROM memos ORDER BY id DESC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut out = Vec::new();
    let mut idx = 1usize;
    let query = query.map(|q| q.to_lowercase());
    for row in rows {
        let cmd = row?;
        let matched = match &query {
            Some(q) => cmd.to_lowercase().contains(q),
            None => true,
        };
        if matched {
            out.push((idx, cmd));
            if out.len() >= limit {
                break;
            }
        }
        idx += 1;
    }
    Ok(out)
}

fn cmd_by_index(conn: &Connection, index: usize) -> rusqlite::Result<Option<String>> {
    if index < 1 {
        return Ok(None);
    }
    conn.query_row(
        "SELECT cmd FROM memos ORDER BY id DESC LIMIT 1 OFFSET ?",
        params![index as i64 - 1],
        |row| row.get(0),
    )
    .optional()
}

fn read_last_history_command() -> Option<String> {
    let histfile = env::var("HISTFILE")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| expand_home("~/.zsh_history"));
    if !histfile.exists() {
        return None;
    }
    let mut file = fs::File::open(histfile).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let content = String::from_utf8_lossy(&buf);
    for line in content.lines().rev() {
        if line.is_empty() {
            continue;
        }
        let mut cmd = line;
        if let Some(rest) = line.strip_prefix(':') {
            if let Some((_, after)) = rest.split_once(';') {
                cmd = after;
            }
        }
        let cmd = cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        if cmd == "memo" || cmd.starts_with("memo ") {
            continue;
        }
        return Some(cmd.to_string());
    }
    None
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        let home = env::var_os("HOME").unwrap_or_default();
        return Path::new(&home).join(stripped);
    }
    PathBuf::from(path)
}

fn is_dangerous(cmd: &str) -> bool {
    let patterns = [
        r"\brm\b",
        r"\bsudo\b",
        r"\bdd\b",
        r"\bmkfs",
        r"\bshutdown\b",
        r"\breboot\b",
        r"\bpoweroff\b",
        r"\|\s*sh\b",
    ];
    patterns.iter().any(|pat| {
        Regex::new(pat)
            .map(|re| re.is_match(cmd))
            .unwrap_or(false)
    })
}

fn confirm_run() -> bool {
    print!("dangerous command, run? [y/N] ");
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

fn which(cmd: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    for path in env::split_paths(&paths) {
        let candidate = path.join(cmd);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn clipboard_command() -> Option<Vec<String>> {
    if cfg!(target_os = "macos") {
        if which("pbcopy").is_some() {
            return Some(vec!["pbcopy".to_string()]);
        }
        return None;
    }
    if which("wl-copy").is_some() {
        return Some(vec!["wl-copy".to_string()]);
    }
    if which("xclip").is_some() {
        return Some(vec!["xclip".to_string(), "-selection".to_string(), "clipboard".to_string()]);
    }
    if which("xsel").is_some() {
        return Some(vec![
            "xsel".to_string(),
            "--clipboard".to_string(),
            "--input".to_string(),
        ]);
    }
    None
}

fn copy_to_clipboard(text: &str) -> bool {
    let cmd = match clipboard_command() {
        Some(cmd) => cmd,
        None => return false,
    };
    let mut child = match Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

fn usage() {
    println!(
        "usage:\n\
  memo                 save last command and list\n\
  memo <query>          list filtered commands\n\
  memo <N>              copy command N\n\
  memo run <N>          execute command N\n\
  memo print <N>        print command N\n\
  memo list [query]     list commands\n\
  memo save [cmd...]    save last or explicit command\n"
    );
}

fn main() -> i32 {
    let args: Vec<String> = env::args().skip(1).collect();
    if matches!(args.get(0).map(String::as_str), Some("-h" | "--help")) {
        usage();
        return 0;
    }

    let conn = match connect_db() {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("db error: {err}");
            return 1;
        }
    };

    if args.is_empty() {
        if let Some(last_cmd) = read_last_history_command() {
            let last_saved = last_saved_cmd(&conn).ok().flatten();
            if last_saved.as_deref() != Some(&last_cmd) {
                let _ = insert_cmd(&conn, &last_cmd);
            }
        }
        let rows = list_cmds(&conn, DEFAULT_LIMIT, None).unwrap_or_default();
        if rows.is_empty() {
            println!("no entries");
            return 0;
        }
        for (idx, cmd) in rows {
            println!("[{idx}] {cmd}");
        }
        return 0;
    }

    match args[0].as_str() {
        "list" => {
            let query = if args.len() > 1 {
                Some(args[1..].join(" "))
            } else {
                None
            };
            let rows = list_cmds(&conn, DEFAULT_LIMIT, query.as_deref()).unwrap_or_default();
            if rows.is_empty() {
                println!("no entries");
                return 0;
            }
            for (idx, cmd) in rows {
                println!("[{idx}] {cmd}");
            }
            return 0;
        }
        "save" => {
            if args.len() > 1 {
                let cmd = args[1..].join(" ");
                if insert_cmd(&conn, &cmd).is_ok() {
                    println!("saved");
                }
                return 0;
            }
            let last_cmd = read_last_history_command();
            if last_cmd.is_none() {
                println!("no history command found");
                return 0;
            }
            if let Some(cmd) = last_cmd {
                let last_saved = last_saved_cmd(&conn).ok().flatten();
                if last_saved.as_deref() != Some(&cmd) {
                    let _ = insert_cmd(&conn, &cmd);
                }
            }
            println!("saved");
            return 0;
        }
        "print" => {
            if args.len() != 2 || args[1].parse::<usize>().is_err() {
                usage();
                return 2;
            }
            let idx = args[1].parse::<usize>().unwrap_or(0);
            match cmd_by_index(&conn, idx).ok().flatten() {
                Some(cmd) => {
                    println!("{cmd}");
                    return 0;
                }
                None => {
                    eprintln!("not found");
                    return 1;
                }
            }
        }
        "run" => {
            if args.len() != 2 || args[1].parse::<usize>().is_err() {
                usage();
                return 2;
            }
            let idx = args[1].parse::<usize>().unwrap_or(0);
            let cmd = match cmd_by_index(&conn, idx).ok().flatten() {
                Some(cmd) => cmd,
                None => {
                    eprintln!("not found");
                    return 1;
                }
            };
            if is_dangerous(&cmd) && !confirm_run() {
                return 1;
            }
            let status = Command::new("sh").arg("-c").arg(&cmd).status();
            return status.ok().and_then(|s| s.code()).unwrap_or(1);
        }
        "_list" => {
            let rows = list_cmds(&conn, DB_CAP, None).unwrap_or_default();
            for (idx, cmd) in rows {
                println!("{idx}\t{cmd}");
            }
            return 0;
        }
        _ => {}
    }

    if args.len() == 1 && args[0].parse::<usize>().is_ok() {
        let idx = args[0].parse::<usize>().unwrap_or(0);
        match cmd_by_index(&conn, idx).ok().flatten() {
            Some(cmd) => {
                if copy_to_clipboard(&cmd) {
                    println!("copied [{idx}]");
                } else {
                    println!("{cmd}");
                    eprintln!("warning: clipboard unavailable");
                }
                return 0;
            }
            None => {
                eprintln!("not found");
                return 1;
            }
        }
    }

    let query = args.join(" ");
    let rows = list_cmds(&conn, DEFAULT_LIMIT, Some(&query)).unwrap_or_default();
    if rows.is_empty() {
        println!("no entries");
        return 0;
    }
    for (idx, cmd) in rows {
        println!("[{idx}] {cmd}");
    }
    0
}
