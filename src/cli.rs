use std::io::Write;

const HELP_STRING: &str  = r#"Usage:

diaryx <COMMAND> <OPTION> [more options...]

Commands:
- create: Creates a new Diaryx file at the path given by <OPTION>
- data: get, edit, or add metadata to a Diaryx file
- export:
"#;

struct Config {
    pub command: String,
    pub option: String,
}

impl Config {
    fn help() {
        println!("{}", HELP_STRING);
        std::process::exit(0);
    }

    pub fn build(args: Vec<String>) -> Config {
        let command = args.get(1)
            .map(|s| s.clone())
            .unwrap_or_else(|| { String::new() });
        let option = args.get(2)
            .map(|s| s.clone())
            .unwrap_or_else(|| { String::new() });
        if command.len() == 0 || option.len() == 0 {
            Config::help();
        }
        Config { command, option }
    }
}

pub struct DiaryxCli {
    config: Config,
}

impl DiaryxCli {
    pub fn from_args() -> DiaryxCli {
        let config = Config::build(std::env::args().collect());
        Self { config: Config { command: config.command, option: config.option } }
    }
    pub fn print_config(&self) {
        println!("Config command: {}", self.config.command);
        println!("Config option: {}", self.config.option);
    }

    fn create(&self) -> Result<(), std::io::Error> {
        println!("Called create with option: {}", self.config.option);
        let mut file = std::fs::File::create_new(self.config.option.as_str())?;
        file.write_all(format!("---\ntitle: {}\n---\n\n# {}\n\n", self.config.option.as_str(), self.config.option.as_str()).as_bytes());
        Ok(())
    }

    pub fn run_command(&self) {
        match self.config.command.as_str() {
            "create" => self.create().unwrap_or_else(|err| {
                eprintln!("Error: {}", err)
            }),
            _ => Config::help(),
        }

    }
}
