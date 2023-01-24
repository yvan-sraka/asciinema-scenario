use asciicast::{Entry, EventType, Header};
use failure::Error;
use html_escape::encode_safe;
use serde::Deserialize;
use serde_json::{from_str, to_string};
use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};
use std::collections::hash_map::DefaultHasher;
use std::fmt::Display;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::exit;
use structopt::StructOpt;
use structopt_flags::{LogLevel, Verbose};
use svg::node::element::{Element, Mask, Rectangle, Text as TextElement};
use svg::node::{NodeDefaultHash, Text, Value};
use svg::{Document, Node};

const TSPAN_TAG: &str = "tspan";

#[derive(Clone, Debug)]
pub struct TSpan {
    inner: Element,
}

impl TSpan {
    pub fn new() -> Self {
        TSpan {
            inner: Element::new(TSPAN_TAG),
        }
    }

    pub fn append<T>(mut self, node: T) -> Self
    where
        T: Node,
    {
        Node::append(&mut self, node);
        self
    }

    #[inline]
    pub fn set<T, U>(mut self, name: T, value: U) -> Self
    where
        T: Into<String>,
        U: Into<Value>,
    {
        Node::assign(&mut self, name, value);
        self
    }

    #[inline]
    pub fn get_inner(&self) -> &Element {
        &self.inner
    }
}

impl Default for TSpan {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeDefaultHash for TSpan {
    fn default_hash(&self, state: &mut DefaultHasher) {
        self.inner.default_hash(state);
    }
}

impl Node for TSpan {
    fn append<T>(&mut self, node: T)
    where
        T: Node,
    {
        self.inner.append(node);
    }

    fn assign<T, U>(&mut self, name: T, value: U)
    where
        T: Into<String>,
        U: Into<Value>,
    {
        self.inner.assign(name, value);
    }
}

impl Display for TSpan {
    fn fmt(&self, formatter: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        self.inner.fmt(formatter)
    }
}

impl From<TSpan> for Element {
    fn from(val: TSpan) -> Self {
        val.inner
    }
}

#[derive(Deserialize, Debug)]
struct ScenarioHeader {
    #[serde(default = "default_step")]
    step: f64,

    #[serde(default = "default_width")]
    width: u32,

    #[serde(default = "default_height")]
    height: u32,
}

fn default_step() -> f64 {
    0.10
}

fn default_width() -> u32 {
    77
}

fn default_height() -> u32 {
    20
}

fn print_entry(entry: Entry) -> Result<(), Error> {
    let s = format!("{:.2}", entry.time);
    let t: f64 = s.parse().unwrap();
    println!(
        "{}",
        to_string(&Entry {
            time: t,
            event_type: entry.event_type,
            event_data: entry.event_data,
        })?
    );
    Ok(())
}

fn clear_terminal(time: &mut f64, step: &f64) -> Result<(), Error> {
    *time += 18.0 * step;
    print_entry(Entry {
        time: *time,
        event_type: EventType::Output,
        event_data: "\r\x1b[2J\r\x1b[H".to_string(),
    })?;
    *time += 3.0 * step;
    Ok(())
}

fn echo_typing(time: &mut f64, step: &f64, line_raw: &str) -> Result<String, Error> {
    let mut bright_applied = false;
    for char in line_raw.to_string().chars() {
        *time += step;
        if char == '#' {
            print_entry(Entry {
                time: *time,
                event_type: EventType::Output,
                event_data: "\x1b[1m".to_string(),
            })?;
            bright_applied = true;
        }
        print_entry(Entry {
            time: *time,
            event_type: EventType::Output,
            event_data: char.to_string(),
        })?;
    }
    // clear
    if bright_applied {
        print_entry(Entry {
            time: *time,
            event_type: EventType::Output,
            event_data: "\x1b[0m".to_string(),
        })?;
    }

    *time += 3.0 * step;
    print_entry(Entry {
        time: *time,
        event_type: EventType::Output,
        event_data: "\r\n".to_string(),
    })?;

    Ok(line_raw.to_string())
}

fn echo_console_line(
    time: &mut f64,
    step: &f64,
    prompt: &str,
    line: &str,
) -> Result<Vec<String>, Error> {
    *time += step;

    let mut preview_lines: Vec<String> = vec![];
    preview_lines.push(prompt.to_string());

    let prompt_line: String = if !prompt.is_empty() {
        format!("\x1b[32m{}\x1b[0m$ ", prompt)
    } else {
        "$ ".to_string()
    };

    print_entry(Entry {
        time: *time,
        event_type: EventType::Output,
        event_data: prompt_line,
    })?;

    *time += 3.0 * step;

    preview_lines.push(echo_typing(time, step, line)?);

    Ok(preview_lines)
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "asciinema-scenario",
    about = "Create asciinema videos from a text file."
)]
struct Cli {
    #[structopt(flatten)]
    verbose: Verbose,

    scenario_file: String,

    #[structopt(name = "preview-file", long, short)]
    svg_preview_file: Option<String>,
}

fn main() -> Result<(), Error> {
    let cli = Cli::from_args();

    // Initialize logging
    let log_level = cli.verbose.get_level_filter();

    // stdout/stderr based logger
    TermLogger::init(
        log_level,            // set log level via "-vvv" flags
        Config::default(),    // how to format logs
        TerminalMode::Stderr, // log to stderr
        ColorChoice::Auto,    // color preference of an end user
    )?;

    // check if does not scenario_file exists
    if !Path::new(&cli.scenario_file).exists() {
        println!(
            "\x1b[31mERROR:\x1b[0m scenario file `{}` does not exist!",
            cli.scenario_file
        );
        exit(1);
    }

    // check if svg_preview_file exists
    if cli.svg_preview_file.is_some() && Path::new(cli.svg_preview_file.as_ref().unwrap()).exists()
    {
        println!(
            "\x1b[31mERROR:\x1b[0m svg preview file `{}` already exist!",
            cli.svg_preview_file.unwrap()
        );
        exit(1);
    }

    // Read lines from scenario_file
    let first_f = File::open(&cli.scenario_file)?;
    let mut first_reader = BufReader::new(first_f);

    // Header
    let mut first_line = String::new();
    first_reader.read_line(&mut first_line)?;

    let header: ScenarioHeader = if let Some(stripped) = first_line.strip_prefix("#! ") {
        from_str(stripped)?
    } else {
        from_str("{}")?
    };
    let asciicast_header = Header {
        version: 2,
        width: header.width,
        height: header.height,
        timestamp: None,
        duration: None,
        idle_time_limit: None,
        command: None,
        title: None,
        env: None,
    };
    println!("{}", to_string(&asciicast_header)?);

    // The rest of the file
    // Read lines from scenario_file
    let mut preview_lines: Vec<Vec<String>> = vec![];
    let f = File::open(cli.scenario_file)?;
    let reader = BufReader::new(f);
    let mut time = 3.0 * header.step;
    for (index, maybe_line) in reader.lines().enumerate() {
        let line = maybe_line?;
        // skip when first line starts with "#! " since we already processed it above
        if index == 0 && line.starts_with("#! ") {
            continue;

        // lines starting with "#timeout: " will create defined timeout
        } else if let Some(stripped) = line.strip_prefix("#timeout:") {
            {
                let timeout: f64 = stripped.trim().parse()?;
                time += timeout;
            }

        // skip lines starting with "#"
        } else if line.starts_with('#') {
            continue;

        // lines starting with "$ " display as console lines
        } else if let Some(stripped) = line.strip_prefix("$ ") {
            preview_lines.push(echo_console_line(&mut time, &header.step, "", stripped)?);

        // lines starting with "(nix-shell) $ " display as console lines
        } else if let Some(stripped) = line.strip_prefix("(nix-shell) $ ") {
            preview_lines.push(echo_console_line(
                &mut time,
                &header.step,
                "(nix-shell) ",
                stripped,
            )?);

        // lines starting with "--" will clear display
        } else if line.starts_with("--") {
            clear_terminal(&mut time, &header.step)?;

        // timeout
        } else if line.trim() == "" {
            time += 3.0 * header.step;

        // everything else print immediately
        } else {
            print_entry(Entry {
                time,
                event_type: EventType::Output,
                event_data: format!("{}\r\n", line.clone()),
            })?;
            preview_lines.push(vec![line.to_string()]);
        }
    }

    match cli.svg_preview_file {
        Some(filename) => {
            let mask_rect = Rectangle::new()
                .set("x", "0")
                .set("y", "0")
                .set("width", "824")
                .set("height", "623")
                .set("fill", "#fff");
            let mask = Mask::new().set("id", "bigterminal-mask").add(mask_rect);
            let rect = Rectangle::new()
                .set("class", "background")
                .set("y", "0")
                .set("x", "0")
                .set("width", "824")
                .set("height", "623");

            let mut text = TextElement::new()
                .set("mask", "url(#bigterminal-mask)")
                .set("transform", "translate(0 0)")
                .set("y", "0")
                .set("x", "0")
                .set("xml:space", "preserve");

            for preview_line in preview_lines.into_iter() {
                let mut tspan = TSpan::new().set("x", "0").set("dy", "1.2em");

                for item in preview_line {
                    if item.is_empty() {
                        tspan = tspan.append(Text::new("$ ".to_string()));
                    } else {
                        let parts: Vec<&str> = item.splitn(2, '#').collect();
                        if parts.len() == 1 {
                            tspan = tspan.append(Text::new(encode_safe(parts[0])));
                        } else {
                            tspan = tspan.append(Text::new(encode_safe(parts[0])));
                            tspan =
                                tspan.append(TSpan::new().set("class", "fg-15").append(Text::new(
                                    encode_safe(parts.clone().split_off(1).join("").as_str()),
                                )));
                        }
                        tspan = tspan.append(
                            TSpan::new()
                                .set("class", "fg-2")
                                .append(Text::new(encode_safe(item.as_str()))),
                        );
                    }
                }
                text = text.add(tspan);
            }

            let svg_preview = Document::new()
                .set("xmlns:dc", "http://purl.org/dc/elements/1.1/")
                .set("xmlns:cc", "http://creativecommons.org/ns#")
                .set("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#")
                .set("xmlns:svg", "http://www.w3.org/2000/svg")
                .set("xmlns", "http://www.w3.org/2000/svg")
                .set("version", "1.1")
                .set("width", "100%")
                .set("viewBox", "0 0 824 623")
                .set("preserveAspectRatio", "xMidYMid meet")
                .add(mask)
                .add(rect)
                .add(text);
            svg::save(filename, &svg_preview)?;
            Ok(())
        }
        None => Ok(()),
    }
}
