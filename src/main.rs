mod lexer;

use std::{
    env, fs,
    io::{self, Write},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

const BUILTINS: [&str; 8] = ["echo", "exit", "type", "pwd", "cd", "export", "unset", "env"];

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
        .unwrap_or(false)
}

fn find_executable(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }

    if command.contains('/') {
        let path = PathBuf::from(command);
        return is_executable_file(&path).then_some(path);
    }

    let path_variable = env::var_os("PATH")?;
    for directory in env::split_paths(&path_variable) {
        let full_path = directory.join(command);
        if is_executable_file(&full_path) {
            return Some(full_path);
        }
    }

    None
}

fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn handle_exit(args: &[String], last_exit_code: i32) -> i32 {
    match args {
        [] => std::process::exit(last_exit_code),
        [code_str] => match code_str.parse::<i32>() {
            Ok(code) => std::process::exit(code),
            Err(_) => {
                eprintln!("exit: {code_str}: numeric argument required");
                std::process::exit(2);
            }
        },
        _ => {
            eprintln!("exit: too many arguments");
            1
        }
    }
}

fn handle_echo(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("-n") {
        print!("{}", args[1..].join(" "));
        let _ = io::stdout().flush();
    } else {
        println!("{}", args.join(" "));
    }
    0
}

fn handle_type(args: &[String]) -> i32 {
    let mut status = 0;
    for arg in args {
        if BUILTINS.contains(&arg.as_str()) {
            println!("{arg} is a shell builtin");
        } else if let Some(full_path) = find_executable(arg) {
            println!("{arg} is {}", full_path.display());
        } else {
            println!("{arg}: not found");
            status = 1;
        }
    }
    status
}

fn handle_pwd() -> i32 {
    match env::current_dir() {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(err) => {
            eprintln!("pwd: {err}");
            1
        }
    }
}

fn handle_cd(args: &[String]) -> i32 {
    if args.len() > 1 {
        eprintln!("cd: too many arguments");
        return 1;
    }

    let is_dash = args.first().map(String::as_str) == Some("-");
    let target = if args.is_empty() || args[0] == "~" {
        match env::var_os("HOME") {
            Some(home) => PathBuf::from(home),
            None => {
                eprintln!("cd: HOME is not set");
                return 1;
            }
        }
    } else if is_dash {
        match env::var_os("OLDPWD") {
            Some(old) => PathBuf::from(old),
            None => {
                eprintln!("cd: OLDPWD is not set");
                return 1;
            }
        }
    } else {
        PathBuf::from(&args[0])
    };

    let old_pwd = env::current_dir().ok();

    match env::set_current_dir(&target) {
        Ok(()) => {
            let new_dir = env::current_dir().ok();
            if is_dash && let Some(ref dir) = new_dir {
                println!("{}", dir.display());
            }
            if let Some(old) = old_pwd {
                unsafe { env::set_var("OLDPWD", old) };
            }
            if let Some(dir) = new_dir {
                unsafe { env::set_var("PWD", dir) };
            }
            0
        }
        Err(_) => {
            eprintln!("cd: {}: No such file or directory", target.display());
            1
        }
    }
}

fn handle_export(args: &[String]) -> i32 {
    if args.is_empty() {
        let mut vars: Vec<(String, String)> = env::vars().collect();
        vars.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in vars {
            println!("export {key}=\"{value}\"");
        }
        return 0;
    }

    let mut status = 0;
    for arg in args {
        match arg.split_once('=') {
            Some((key, value)) => {
                if is_valid_var_name(key) {
                    unsafe { env::set_var(key, value) };
                } else {
                    eprintln!("export: `{arg}': not a valid identifier");
                    status = 1;
                }
            }
            None => {
                if !is_valid_var_name(arg) {
                    eprintln!("export: `{arg}': not a valid identifier");
                    status = 1;
                }
            }
        }
    }
    status
}

fn handle_unset(args: &[String]) -> i32 {
    let mut status = 0;
    for arg in args {
        if is_valid_var_name(arg) {
            unsafe { env::remove_var(arg) };
        } else {
            eprintln!("unset: `{arg}': not a valid identifier");
            status = 1;
        }
    }
    status
}

fn handle_env() -> i32 {
    for (key, value) in env::vars() {
        println!("{key}={value}");
    }
    0
}

fn execute_external(program_name: &str, args: &[String]) -> i32 {
    let Some(full_path) = find_executable(program_name) else {
        println!("{program_name}: command not found");
        return 127;
    };

    match Command::new(full_path)
        .arg0(program_name)
        .args(args)
        .status()
    {
        Ok(status) => status.code().unwrap_or(128),
        Err(error) => {
            eprintln!("{program_name}: {error}");
            126
        }
    }
}

fn main() {
    let mut last_exit_code: i32 = 0;

    loop {
        print!("$ ");
        if let Err(err) = io::stdout().flush() {
            eprintln!("rush: flush error: {err}");
            break;
        }

        let mut input = String::new();
        let bytes_read = match io::stdin().read_line(&mut input) {
            Ok(n) => n,
            Err(err) => {
                eprintln!("rush: read error: {err}");
                break;
            }
        };

        if bytes_read == 0 {
            break;
        }

        let line = input.trim_end_matches(&['\r', '\n'][..]);
        let tokens = match lexer::tokenize(line, last_exit_code) {
            Ok(tokens) => tokens,
            Err(err) => {
                eprintln!("rush: {err}");
                last_exit_code = 2;
                continue;
            }
        };

        if tokens.is_empty() {
            continue;
        }

        let all_assignments = tokens.iter().all(|tok| {
            tok.split_once('=')
                .is_some_and(|(key, _)| is_valid_var_name(key))
        });

        if all_assignments {
            for tok in &tokens {
                if let Some((key, value)) = tok.split_once('=') {
                    unsafe { env::set_var(key, value) };
                }
            }
            last_exit_code = 0;
            continue;
        }

        let program_name = &tokens[0];
        let args = &tokens[1..];

        last_exit_code = match program_name.as_str() {
            "exit" => handle_exit(args, last_exit_code),
            "echo" => handle_echo(args),
            "type" => handle_type(args),
            "pwd" => handle_pwd(),
            "cd" => handle_cd(args),
            "export" => handle_export(args),
            "unset" => handle_unset(args),
            "env" => handle_env(),
            _ => execute_external(program_name, args),
        };
    }
}
