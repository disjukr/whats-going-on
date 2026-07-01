use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct IntegratedShellLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn integrate_shell_launch(
    root_dir: &Path,
    command: &str,
    args: &[String],
) -> IntegratedShellLaunch {
    match shell_kind(command) {
        Some(ShellKind::Bash) if !has_command_arg(args) => integrate_bash(root_dir, command, args),
        Some(ShellKind::Fish) if !has_command_arg(args) => integrate_fish(root_dir, command, args),
        Some(ShellKind::Zsh) if !has_command_arg(args) => integrate_zsh(root_dir, command, args),
        _ => IntegratedShellLaunch {
            command: command.to_string(),
            args: args.to_vec(),
            env: Vec::new(),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellKind {
    Bash,
    Fish,
    Zsh,
}

fn integrate_bash(root_dir: &Path, command: &str, args: &[String]) -> IntegratedShellLaunch {
    let Some(script) = ensure_script(root_dir, "bash", BASH_INTEGRATION_SCRIPT) else {
        return unchanged(command, args);
    };
    let (filtered_args, login) = filter_bash_args(args);
    let mut next_args = vec![
        "--init-file".to_string(),
        script.to_string_lossy().into_owned(),
    ];
    if !filtered_args.iter().any(|arg| arg == "-i") {
        next_args.push("-i".to_string());
    }
    next_args.extend(filtered_args);
    let mut env = Vec::new();
    if login {
        env.push(("WGO_SHELL_LOGIN".to_string(), "1".to_string()));
    }
    IntegratedShellLaunch {
        command: command.to_string(),
        args: next_args,
        env,
    }
}

fn integrate_fish(root_dir: &Path, command: &str, args: &[String]) -> IntegratedShellLaunch {
    let Some(script) = ensure_script(root_dir, "fish", FISH_INTEGRATION_SCRIPT) else {
        return unchanged(command, args);
    };
    let mut next_args = vec![
        "--init-command".to_string(),
        format!("source {}", fish_single_quote(&script.to_string_lossy())),
    ];
    next_args.extend(args.iter().cloned());
    IntegratedShellLaunch {
        command: command.to_string(),
        args: next_args,
        env: Vec::new(),
    }
}

fn integrate_zsh(root_dir: &Path, command: &str, args: &[String]) -> IntegratedShellLaunch {
    let Some(script) = ensure_script(root_dir, "zsh", ZSH_INTEGRATION_SCRIPT) else {
        return unchanged(command, args);
    };
    let Some(zdotdir) = ensure_zsh_zdotdir(root_dir, &script) else {
        return unchanged(command, args);
    };
    let user_zdotdir = std::env::var("ZDOTDIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_default();
    IntegratedShellLaunch {
        command: command.to_string(),
        args: args.to_vec(),
        env: vec![
            (
                "ZDOTDIR".to_string(),
                zdotdir.to_string_lossy().into_owned(),
            ),
            ("WGO_USER_ZDOTDIR".to_string(), user_zdotdir),
        ],
    }
}

fn unchanged(command: &str, args: &[String]) -> IntegratedShellLaunch {
    IntegratedShellLaunch {
        command: command.to_string(),
        args: args.to_vec(),
        env: Vec::new(),
    }
}

fn shell_kind(command: &str) -> Option<ShellKind> {
    let file_name = PathBuf::from(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();
    let stem = file_name.strip_suffix(".exe").unwrap_or(&file_name);
    match stem {
        "bash" => Some(ShellKind::Bash),
        "fish" => Some(ShellKind::Fish),
        "zsh" => Some(ShellKind::Zsh),
        _ => None,
    }
}

fn has_command_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "-c"
            || arg == "--command"
            || arg.starts_with("--command=")
            || is_short_option_with(arg, 'c')
    })
}

fn filter_bash_args(args: &[String]) -> (Vec<String>, bool) {
    let mut filtered = Vec::new();
    let mut login = false;
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            "--login" | "-l" => {
                login = true;
            }
            "--rcfile" | "--init-file" => {
                skip_next = true;
            }
            "-i" => filtered.push(arg.clone()),
            _ if arg.starts_with("--rcfile=") || arg.starts_with("--init-file=") => {}
            _ if is_short_option_with(arg, 'l') => {
                login = true;
                if is_short_option_with(arg, 'i') {
                    filtered.push("-i".to_string());
                }
            }
            _ => filtered.push(arg.clone()),
        }
    }
    (filtered, login)
}

fn is_short_option_with(arg: &str, option: char) -> bool {
    arg.starts_with('-')
        && !arg.starts_with("--")
        && arg.len() > 2
        && arg.chars().skip(1).any(|item| item == option)
}

fn ensure_script(root_dir: &Path, name: &str, content: &str) -> Option<PathBuf> {
    let path = root_dir.join(format!("wgo-shell-integration-{name}"));
    write_if_changed(&path, content).ok()?;
    Some(path)
}

fn ensure_zsh_zdotdir(root_dir: &Path, script: &Path) -> Option<PathBuf> {
    let dir = root_dir.join("zsh-zdotdir");
    fs::create_dir_all(&dir).ok()?;
    let zshrc = format!(
        r#"if [ -n "${{WGO_USER_ZDOTDIR:-}}" ] && [ -r "${{WGO_USER_ZDOTDIR}}/.zshrc" ]; then
  source "${{WGO_USER_ZDOTDIR}}/.zshrc"
fi
source {}
"#,
        sh_single_quote(&script.to_string_lossy()),
    );
    write_if_changed(&dir.join(".zshrc"), &zshrc).ok()?;
    Some(dir)
}

fn write_if_changed(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if matches!(fs::read_to_string(path), Ok(current) if current == content) {
        return Ok(());
    }
    fs::write(path, content)
}

fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn fish_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', r"\\").replace('\'', r"\'"))
}

const BASH_INTEGRATION_SCRIPT: &str = r#"
if [ -n "${WGO_SHELL_INTEGRATION:-}" ]; then
  return
fi
export WGO_SHELL_INTEGRATION=1

if [ "${WGO_SHELL_LOGIN:-}" = "1" ]; then
  [ -r /etc/profile ] && . /etc/profile
  if [ -r ~/.bash_profile ]; then
    . ~/.bash_profile
  elif [ -r ~/.bash_login ]; then
    . ~/.bash_login
  elif [ -r ~/.profile ]; then
    . ~/.profile
  fi
else
  [ -r ~/.bashrc ] && . ~/.bashrc
fi
unset WGO_SHELL_LOGIN

__wgo_escape_value() {
  local value="${1//\\/\\\\}"
  value="${value//;/\\x3b}"
  printf '%s' "$value"
}

__wgo_prompt_start() { printf '\e]633;A\a'; }
__wgo_prompt_end() { printf '\e]633;B\a'; }
__wgo_update_cwd() { printf '\e]633;P;Cwd=%s\a' "$(__wgo_escape_value "$PWD")"; }

__wgo_prompt_command() {
  local status="$?"
  if [ "${__wgo_seen_prompt:-0}" = "1" ]; then
    printf '\e]633;D;%s\a' "$status"
  fi
  __wgo_seen_prompt=1
  __wgo_update_cwd
  if [ -n "${__wgo_original_prompt_command:-}" ]; then
    eval "$__wgo_original_prompt_command"
  fi
  return "$status"
}

__wgo_original_prompt_command="${PROMPT_COMMAND:-}"
PROMPT_COMMAND=__wgo_prompt_command
PS1="\[$(__wgo_prompt_start)\]${PS1:-\\s-\\v\\\$ }\[$(__wgo_prompt_end)\]"
"#;

const FISH_INTEGRATION_SCRIPT: &str = r#"
if set -q WGO_SHELL_INTEGRATION
  return
end
set -gx WGO_SHELL_INTEGRATION 1

function __wgo_escape_value --argument-names value
  set value (string replace --all '\' '\\' -- "$value")
  set value (string replace --all ';' '\x3b' -- "$value")
  printf '%s' "$value"
end

function __wgo_esc
  printf "\e]633;%s\a" (string join ';' -- $argv)
end

function __wgo_cmd_executed --on-event fish_preexec
  __wgo_esc E (__wgo_escape_value "$argv")
  __wgo_esc C
end

function __wgo_cmd_finished --on-event fish_postexec
  __wgo_esc D $status
end

function __wgo_update_cwd --on-event fish_prompt
  __wgo_esc P Cwd=(__wgo_escape_value "$PWD")
end

if functions --query fish_prompt
  functions --copy fish_prompt __wgo_original_fish_prompt
else
  function __wgo_original_fish_prompt
    printf '%s@%s %s> ' (whoami) (prompt_hostname) (prompt_pwd)
  end
end

function fish_prompt
  __wgo_esc A
  __wgo_original_fish_prompt
  __wgo_esc B
end
"#;

const ZSH_INTEGRATION_SCRIPT: &str = r#"
if [ -n "${WGO_SHELL_INTEGRATION:-}" ]; then
  return
fi
export WGO_SHELL_INTEGRATION=1

autoload -Uz add-zsh-hook

__wgo_escape_value() {
  local value="${1//\\/\\\\}"
  value="${value//;/\\x3b}"
  printf '%s' "$value"
}

__wgo_prompt_start() { printf '\e]633;A\a'; }
__wgo_prompt_end() { printf '\e]633;B\a'; }
__wgo_update_cwd() { printf '\e]633;P;Cwd=%s\a' "$(__wgo_escape_value "$PWD")"; }

__wgo_precmd() {
  local status="$?"
  if [ "${__wgo_seen_prompt:-0}" = "1" ]; then
    printf '\e]633;D;%s\a' "$status"
  fi
  __wgo_seen_prompt=1
  __wgo_update_cwd
}

__wgo_preexec() {
  printf '\e]633;E;%s\a' "$(__wgo_escape_value "$1")"
  printf '\e]633;C\a'
}

add-zsh-hook precmd __wgo_precmd
add-zsh-hook preexec __wgo_preexec

PS1="%{$(__wgo_prompt_start)%}${PS1:-%m%# }%{$(__wgo_prompt_end)%}"
"#;
