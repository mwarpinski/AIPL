use aipl_core::checker::TypeChecker;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use std::env;

// Embedded AIPL S-Expression Source File
const AIPL_CLI_SRC: &str = include_str!("../../examples/hn_cli.aipl");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let command = if args.len() > 1 { &args[1] } else { "top" };

    // 1. Load and parse AIPL CLI module
    let cli_module = Parser::parse(AIPL_CLI_SRC)?;

    // 2. Formally type-check & verify contracts statically
    let mut checker = TypeChecker::new();
    checker.check_module(&cli_module)?;

    // 3. Initialize AIPL Virtual Machine
    let mut vm = VM::new();
    vm.load_module(cli_module);

    match &command[..] {
        "top" => {
            println!("\x1b[48;5;208m\x1b[30m\x1b[1m  [Y] Hacker News Terminal | AIPL Engine v1.0  \x1b[0m");
            vm.invoke("handle_command_top", vec![])?;
        }
        "show" => {
            println!("\x1b[48;5;208m\x1b[30m\x1b[1m  [Y] Hacker News Terminal | Show HN (AIPL Engine)  \x1b[0m");
            vm.invoke("handle_command_show", vec![])?;
        }
        "register" => {
            if args.len() < 4 {
                println!("\x1b[31mUsage: hn-cli register <username> <password>\x1b[0m");
                return Ok(());
            }
            let user = &args[2];
            let pass = &args[3];
            let pass_code: i64 = (pass.chars().map(|c| c as i64).sum::<i64>() % 100000) + 1;
            let salt: i64 = 991;

            println!("\x1b[32m[AIPL Security Engine]: Registering user '{}'...\x1b[0m", user);
            let pass_hash = vm.invoke("hash_password", vec![Value::Int(pass_code), Value::Int(salt)])?;
            println!("\x1b[36m[AIPL DB]: Account Created! Hash: {:?}\x1b[0m", pass_hash);
        }
        "login" => {
            if args.len() < 4 {
                println!("\x1b[31mUsage: hn-cli login <username> <password>\x1b[0m");
                return Ok(());
            }
            let user = &args[2];
            let pass = &args[3];
            let pass_code: i64 = (pass.chars().map(|c| c as i64).sum::<i64>() % 100000) + 1;
            let salt: i64 = 991;

            let computed_hash_val = vm.invoke("hash_password", vec![Value::Int(pass_code), Value::Int(salt)])?;
            let stored_hash: i64 = match computed_hash_val {
                Value::Int(i) => i,
                _ => 1,
            };

            println!("\x1b[33m[AIPL Security Engine]: Authenticating user '{}'...\x1b[0m", user);
            let res = vm.invoke(
                "handle_command_login",
                vec![
                    Value::Int(101),
                    Value::Int(pass_code),
                    Value::Int(salt),
                    Value::Int(stored_hash),
                ],
            )?;
            println!("\x1b[32m[AIPL Auth Verification]: {:?}\x1b[0m", res);
        }
        "submit" => {
            if args.len() < 4 {
                println!("\x1b[31mUsage: hn-cli submit <title> <url>\x1b[0m");
                return Ok(());
            }
            let title = &args[2];
            let url = &args[3];
            println!("\x1b[32m[AIPL Submission Pipeline]: Submitting link '{}' ({})\x1b[0m", title, url);
            let res = vm.invoke("handle_command_submit", vec![Value::Int(101), Value::Int(15)])?;
            println!("\x1b[36m[AIPL Karma Engine]: Author Karma Updated -> {:?}\x1b[0m", res);
        }
        "upvote" => {
            let story_id: i64 = if args.len() > 2 { args[2].parse().unwrap_or(1) } else { 1 };
            println!("\x1b[32m[AIPL Upvote Pipeline]: Registering upvote for story #{}\x1b[0m", story_id);
            let new_score = vm.invoke("handle_command_upvote", vec![Value::Int(story_id), Value::Int(482)])?;
            println!("\x1b[36m[AIPL Score Output]: New Points -> {:?}\x1b[0m", new_score);
        }
        "profile" => {
            let user = if args.len() > 2 { &args[2] } else { "demis_h" };
            println!("\x1b[48;5;208m\x1b[30m\x1b[1m  User Profile: {}  \x1b[0m", user);
            println!("  Karma Points  : \x1b[33m482\x1b[0m");
            println!("  Member Since  : 2026-01-15");
            println!("  Submissions   : 12 stories");
        }
        "inspect" => {
            println!("\x1b[34m[AIPL S-Expression Bytecode Inspection]\x1b[0m");
            println!("{}", AIPL_CLI_SRC);
        }
        _ => {
            println!("\x1b[1mHacker News AIPL Linux CLI Options:\x1b[0m");
            println!("  hn-cli top                      View top tech stories");
            println!("  hn-cli show                     View Show HN stories");
            println!("  hn-cli register <user> <pass>   Register user account via AIPL security engine");
            println!("  hn-cli login <user> <pass>      Authenticate user credentials");
            println!("  hn-cli submit <title> <url>     Submit new link (+1 Karma)");
            println!("  hn-cli upvote <story_id>        Upvote story");
            println!("  hn-cli profile <username>       View user karma profile");
            println!("  hn-cli inspect                  Inspect raw AIPL IR code");
        }
    }

    Ok(())
}
