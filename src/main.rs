// This code may not be used for any purpose. Be gay, do crime.

#[macro_use]
extern crate clap;

use clap::{App, ArgMatches};
use std::error::Error;
use env_logger::{Env, from_env};
use bandsocks_runtime::Container;

fn main() -> Result<(), Box<dyn Error>> {
    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    let log_level = matches.value_of("log_level").unwrap();
    from_env(Env::default().default_filter_or(log_level)).init();

    let run_args = string_values(&matches, "run_args");
    let image_reference = matches.value_of("image_reference").unwrap().parse()?;

    let _c = Container::pull(&image_reference)?
        .args(run_args)
        .spawn()?;

    Ok(())        
}

fn string_values<S: AsRef<str>>(matches: &ArgMatches, name: S) -> Vec<String> {
    match matches.values_of(name) {
        Some(strs) => strs.map(|s| s.to_string()).collect(),
        None => Vec::new(),
    }
}
