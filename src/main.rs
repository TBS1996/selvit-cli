use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

use alloc::vec::Vec;
use chrono::{Days, TimeDelta, prelude::*};
use nonempty::NonEmpty;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
extern crate alloc;
use nonempty::nonempty;

type Unix = i64;

fn get_seed(id: Uuid, day: NaiveDate) -> u64 {
    let id = id.as_u64_pair().0;
    let start = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
    let days_since = day.signed_duration_since(start).num_days();
    (days_since as u64).wrapping_add(id)
}

fn calc_dosage(input: &Input, day: NaiveDate) -> u32 {
    let seed = get_seed(input.id, day);
    match &input.ty {
        InputType::Boolean => {
            let mut rng = StdRng::seed_from_u64(seed);
            let random_number: bool = rng.r#gen();
            random_number as u32
        }
        InputType::Dosage {
            min: _,
            max: _,
            unit_name: _,
            units,
        } => {
            let mut rng = StdRng::seed_from_u64(seed);

            let unit = units.first();

            if let Some((min_units, max_units)) = unit_dose_bounds(unit, &input.ty) {
                let range = max_units.saturating_sub(min_units) + 1;
                let num_units = min_units + (rng.r#gen::<u32>() % range);

                return num_units;
            }

            panic!()
        }
    }
}

fn unit_dose_bounds(unit: &Unit, dosage: &InputType) -> Option<(u32, u32)> {
    match dosage {
        InputType::Dosage { min, max, .. } => {
            if unit.dose <= 0.0 {
                return None;
            }

            let min_units = (*min / unit.dose).ceil() as u32;
            let max_units = (*max / unit.dose).floor() as u32;

            if min_units > max_units {
                None
            } else {
                Some((min_units, max_units))
            }
        }
        _ => None,
    }
}

fn print_outputs(inputs: &Vec<Input>, day: NaiveDate) {
    // Step 1: Determine max input name length
    let max_name_len = inputs.iter().map(|i| i.name.len()).max().unwrap_or(0);
    let now = Local::now().time();

    // Step 2: Print each input with padding
    for (idx, input) in inputs.iter().enumerate() {
        if let Some(after) = &input.valid_after {
            if now < *after {
                continue;
            }
        }
        let dosage = calc_dosage(&input, day);

        let idx = if idx < 10 {
            format!(" {idx}")
        } else {
            idx.to_string()
        };

        match &input.ty {
            InputType::Boolean => {
                let padded_name = format!("{:width$}", input.name, width = max_name_len);
                let result = if dosage == 0 { "no" } else { "yes" };
                println!("{idx}:   {} {}", padded_name, result);
            }
            InputType::Dosage { units, .. } => {
                let consumed: u32 = Log::load_for_day(day)
                    .iter()
                    .filter_map(|log| {
                        if log.source == units.first().id {
                            Some(log.quantity)
                        } else {
                            None
                        }
                    })
                    .sum();

                let emoji = if consumed >= dosage { "✅" } else { "  " };

                let padded_name = format!("{:width$}", input.name, width = max_name_len);
                println!(
                    "{idx}: {}{} {}/{} {}",
                    emoji,
                    padded_name,
                    consumed,
                    dosage,
                    units.first().name,
                );
            }
        }
    }
}

fn print_day(day: NaiveDate) {
    let inputs = Input::load_all();

    println!("dosages on date: {day}");
    println!();
    print_outputs(&inputs, day);
}

use std::env;

fn run() -> Option<()> {
    let mut full_cmd = env::args().collect::<Vec<_>>().into_iter();
    full_cmd.next(); // ignore program invocation

    match full_cmd.next().as_deref() {
        Some("-3") => {
            let today = Local::now().date_naive();
            let yesterday = today.checked_sub_days(Days::new(1)).unwrap();
            let tomorrow = today.checked_add_days(Days::new(1)).unwrap();

            for day in [yesterday, today, tomorrow] {
                print_day(day);
                println!();
                println!();
            }
        }
        Some("after") => {
            let idx: usize = full_cmd.next()?.parse().ok()?;
            let after: Option<usize> = full_cmd.next().and_then(|x| x.parse().ok());
            let mut input = Input::load_all().get(idx).cloned()?;
            let after = after
                .map(|hr| NaiveTime::from_hms_opt(hr as u32, 0, 0))
                .flatten();

            input.valid_after = after;
            input.save();
        }
        Some("w") => {
            let today = Local::now().date_naive();

            for i in 0..7 {
                let day = today.checked_add_days(Days::new(i)).unwrap();
                print_day(day);
                println!();
                println!();
            }
        }
        Some("m") => {
            let today = Local::now().date_naive();

            for i in 0..30 {
                let day = today.checked_add_days(Days::new(i)).unwrap();
                print_day(day);
                println!();
                println!();
            }
        }
        Some("booladd") => {
            let name = full_cmd.next().or_else(|| get_input("name"))?;
            let input = Input::new_boolean(name);
            input.save();
        }
        Some("add") => {
            let name = full_cmd.next().or_else(|| get_input("name"))?;
            let unit_name: String = get_input("unit name (plural, e.g., grams/minutes")?;
            let min_dose: f32 = typed_input(&format!("minimum dose of {} {}", &unit_name, &name))?;
            let max_dose: f32 = typed_input(&format!("max dose of {} {}", &unit_name, &name))?;

            let unit = {
                let source_name = get_input("source name")?;
                let prompt = format!("how much {} in one {}?", &unit_name, &source_name);
                let dose: f32 = typed_input(&prompt)?;
                Unit {
                    name: source_name,
                    id: Uuid::new_v4(),
                    dose,
                }
            };

            let input = Input::new_dosage(name, unit_name, min_dose, max_dose, nonempty![unit]);
            println!("new input added!");
            input.save();
        }
        Some("log") => {
            let idx: usize = full_cmd.next()?.parse().ok()?;
            let input = Input::load_all().get(idx)?.clone();
            match input.ty {
                InputType::Boolean => return Some(()),
                InputType::Dosage { units, .. } => {
                    let qty: u32 = full_cmd.next()?.parse().ok()?;
                    let time = match full_cmd.next() {
                        Some(hrs_ago) => {
                            let hrs_ago: f32 = hrs_ago.parse().ok()?;
                            let secs_ago = (hrs_ago * 3600.) as i64;
                            Local::now()
                                .checked_sub_signed(TimeDelta::seconds(secs_ago))
                                .unwrap()
                        }
                        None => Local::now(),
                    };

                    let source = units.first();
                    let log = Log::new(source.id, qty, time);
                    log.save();
                }
            }
        }
        Some("refresh") => {
            refresh();
        }
        Some(_) => {}
        None => {
            let today = Local::now().date_naive();
            print_day(today);
        }
    }

    Some(())
}

fn main() {
    run();
}

use std::io;

fn typed_input<T: FromStr>(prompt: &str) -> Option<T> {
    loop {
        match get_input(prompt).map(|s| T::from_str(&s)) {
            Some(Ok(t)) => return Some(t),
            Some(Err(_)) => continue,
            None => return None,
        }
    }
}

fn get_input(prompt: &str) -> Option<String> {
    println!();
    print!("{prompt}: ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    let input = input.trim().to_string();
    (!input.is_empty()).then_some(input)
}

#[derive(Clone, Serialize, Deserialize)]
struct Input {
    id: Uuid,
    name: String,
    ty: InputType,
    valid_after: Option<NaiveTime>,
}

fn root_dir() -> PathBuf {
    dirs::data_local_dir().unwrap().join("selvit")
}

impl Input {
    fn path_dir() -> PathBuf {
        let p = root_dir().join("inputs");
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn path(&self) -> PathBuf {
        Self::path_dir().join(&self.name)
    }

    fn load_all() -> Vec<Input> {
        let path = Self::path_dir();

        let mut doses = vec![];
        let mut bools = vec![];

        for entry in fs::read_dir(&path).unwrap() {
            let p = entry.unwrap().path();
            let s = fs::read_to_string(&p).unwrap();
            let input: Input = toml::from_str(&s).unwrap();
            if input.ty.is_bool() {
                bools.push(input);
            } else {
                doses.push(input);
            }
        }

        doses.sort_by_key(|i| i.name.clone());
        bools.sort_by_key(|i| i.name.clone());

        doses.extend(bools);
        doses
    }

    fn save(&self) {
        let created_new =
            if let Some(input) = Input::load_all().iter().find(|input| input.id == self.id) {
                fs::remove_file(&input.path()).unwrap();
                false
            } else {
                true
            };

        let s = toml::to_string(self).unwrap();
        let path = Self::path_dir().join(&self.name);
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(s.as_bytes()).unwrap();

        let op = if created_new {
            Op::NewInput(self.name.clone())
        } else {
            Op::UpdateInput(self.name.clone())
        };

        match (created_new, git_push(&root_dir(), op)) {
            (true, Ok(_)) => println!("saved and pushed new input"),
            (true, Err(_)) => println!("saved new input"),
            (false, Ok(_)) => println!("updated and pushed input"),
            (false, Err(_)) => println!(""),
        }
    }

    #[allow(dead_code)]
    fn new_boolean(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            ty: InputType::Boolean,
            valid_after: None,
        }
    }

    #[allow(dead_code)]
    fn new_dosage(
        name: String,
        unit_name: String,
        min: f32,
        max: f32,
        units: NonEmpty<Unit>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            valid_after: None,
            ty: InputType::Dosage {
                min,
                max,
                unit_name,
                units,
            },
        }
    }
}

fn refresh() {
    println!("refreshing inputs");
    let inputs = Input::load_all();
    let path = Input::path_dir();
    fs::remove_dir_all(&path).unwrap();
    fs::create_dir_all(&path).unwrap();
    for input in inputs {
        input.save();
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum InputType {
    Boolean,
    Dosage {
        min: f32,
        max: f32,
        unit_name: String,
        units: NonEmpty<Unit>,
    },
}

impl InputType {
    fn is_bool(&self) -> bool {
        match self {
            InputType::Boolean => true,
            InputType::Dosage { .. } => false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Unit {
    name: String,
    #[serde(default = "Uuid::new_v4")]
    id: Uuid,
    dose: f32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Log {
    source: Uuid,
    time: Unix,
    quantity: u32,
}

fn load_source(source: Uuid) -> Unit {
    for input in Input::load_all() {
        match input.ty {
            InputType::Boolean => continue,
            InputType::Dosage { units, .. } => {
                if let Some(unit) = units.into_iter().find(|unit| unit.id == source) {
                    return unit;
                }
            }
        }
    }

    panic!()
}

fn input_from_source(source: Uuid) -> Input {
    Input::load_all()
        .into_iter()
        .find(|input| match input.ty.clone() {
            InputType::Boolean => false,
            InputType::Dosage { units, .. } => {
                units.iter().find(|unit| unit.id == source).is_some()
            }
        })
        .unwrap()
}

impl Log {
    fn new(source: Uuid, quantity: u32, time: DateTime<Local>) -> Self {
        Self {
            source,
            time: time.timestamp(),
            quantity,
        }
    }

    fn path() -> PathBuf {
        let p = root_dir().join("log");
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn save(&self) {
        let s = toml::to_string(self).unwrap();
        let path = Self::path().join(self.time.to_string());
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(s.as_bytes()).unwrap();

        let op = Op::Log {
            input: input_from_source(self.source).name,
            source: load_source(self.source).name,
            qty: self.quantity as usize,
        };

        if git_push(&root_dir(), op).is_ok() {
            println!("saved and pushed log");
        } else {
            println!("saved log");
        }
    }

    fn load_all() -> Vec<Self> {
        let path = Self::path();

        let mut out = vec![];
        for entry in fs::read_dir(&path).unwrap() {
            let p = entry.unwrap().path();
            let s = fs::read_to_string(&p).unwrap();
            let input: Self = toml::from_str(&s).unwrap();
            out.push(input)
        }
        out.sort_by_key(|i| i.time);
        out
    }

    fn load_for_day(day: NaiveDate) -> Vec<Self> {
        let day_start = new_day_unix(day);
        let day_end = new_day_unix(day.checked_add_days(Days::new(1)).unwrap());

        let mut day_logs: Vec<Self> = vec![];

        for log in Self::load_all() {
            if log.time > day_end {
                break;
            } else if log.time < day_start {
                continue;
            } else {
                day_logs.push(log);
            }
        }

        day_logs
    }
}

fn new_day_unix(day: NaiveDate) -> i64 {
    let time = NaiveTime::from_hms_opt(3, 0, 0).unwrap();
    let naive_dt = day.and_time(time);
    let local_dt = Local.from_local_datetime(&naive_dt).unwrap();
    local_dt.timestamp()
}

use std::path::Path;
use std::process::Command;

enum Op {
    NewInput(String),
    UpdateInput(String),
    Log {
        input: String,
        source: String,
        qty: usize,
    },
}

use std::process::Stdio;

fn git_push(path: &Path, op: Op) -> Result<(), Box<dyn std::error::Error>> {
    let msg = match op {
        Op::NewInput(name) => format!("add new input: {name}"),
        Op::UpdateInput(name) => format!("update input: {name}"),
        Op::Log { input, source, qty } => format!("logging {qty} {source} for {input}"),
    };

    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("add")
        .arg(".")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("git add failed".into());
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("--porcelain")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;

    if output.stdout.is_empty() {
        return Err("nothing to commit".into());
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("commit")
        .arg("-m")
        .arg(&msg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("git commit failed".into());
    }

    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("push")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if !status.success() {
        return Err("git push failed".into());
    }

    Ok(())
}
