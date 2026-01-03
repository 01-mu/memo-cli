# memo zsh integration
# source this file from .zshrc

_memo_select_command() {
  local pick idx cmd
  if (( $+commands[fzf] )); then
    pick="$(memo _list | fzf --delimiter=$'\t' --with-nth=2.. --prompt='memo> ')"
    [[ -z "$pick" ]] && return 1
    idx="${pick%%$'\t'*}"
  else
    print -r -- ""
    memo list
    read -r "idx?memo number: "
  fi

  if [[ "$idx" =~ '^[0-9]+$' ]]; then
    cmd="$(memo print "$idx")"
    [[ -n "$cmd" ]] || return 1
    LBUFFER="$cmd"
    RBUFFER=""
    return 0
  fi
  return 1
}

_memo_space_widget() {
  if [[ "$LBUFFER" == "memo" && -z "$RBUFFER" ]]; then
    if ! _memo_select_command; then
      LBUFFER="memo "
      RBUFFER=""
    fi
    zle redisplay
  else
    zle self-insert
  fi
}

zle -N _memo_space_widget
bindkey ' ' _memo_space_widget
