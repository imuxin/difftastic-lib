#[macro_use]
extern crate log;

extern crate pretty_env_logger;

mod constants;
mod diff;
mod display;
mod files;
mod line_parser;
mod lines;
mod options;
mod parse;
mod positions;
mod summary;

pub mod mainfn;
