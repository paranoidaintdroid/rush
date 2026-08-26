use std::{
    env,
    fs,
    io::{self, Write},
    os::unix::{
        fs::PermissionsExt,
        process::CommandExt,
    },
    path::PathBuf,
    process::Command,
};

fn find_executable(command: &str) -> Option<PathBuf> {
    let path_variable = env::var_os("PATH")?;

    for directory in env::split_paths(&path_variable) {
        let full_path = directory.join(command);

        let metadata = match fs::metadata(&full_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let is_regular_file = metadata.is_file();
        let is_executable = metadata.permissions().mode() & 0o111 != 0;

        if is_regular_file && is_executable {
            return Some(full_path);
        }
    }

    None
}

fn main() {
    let built_in: [&str; 3] = ["echo", "exit", "type"];

    loop {
        print!("$ ");
        io::stdout().flush().unwrap();

        let mut command = String::new();

        io::stdin().read_line(&mut command).unwrap();

        let command = command.trim();

        if command == "exit" {
            break;
        } else if command.starts_with("echo ") {
            println!("{}", &command[5..]);
        } else if command.starts_with("type ") {
            let to_search = &command[5..];

            if built_in.contains(&to_search) {
                println!("{to_search} is a shell builtin");
            } else if let Some(full_path) = find_executable(to_search) {
                println!("{to_search} is {}", full_path.display());
            } else {
                println!("{to_search}: not found");
            }
        } else {
            let mut parts = command.split_whitespace();

            let program_name = match parts.next() {
                Some(name) => name,
                None => continue,
            };

            let arguments = parts;

            if let Some(full_path) = find_executable(program_name) {
                let result = Command::new(full_path)
                    .arg0(program_name)
                    .args(arguments)
                    .status();

                if let Err(error) = result {
                    eprintln!("{program_name}: {error}");
                }
            } else {
                println!("{program_name}: command not found");
            }
        }
    }
}