use chrono::NaiveDate;
use rusqlite::{params, Connection};
use std::error::Error;
use std::fs::File;
use std::io::{stdout, Write};
use std::iter::Iterator;
use std::path::PathBuf;
use std::time::Instant;
use std::{env, fs};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug, Clone)]
pub struct Verdict {
    pub ok: bool,
    pub msg: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub time_limit: f64,
    pub public_wall_time: bool,
}

impl From<bool> for Verdict {
    fn from(ok: bool) -> Self {
        Verdict { ok, msg: None }
    }
}

impl From<(bool, Option<String>)> for Verdict {
    fn from((ok, msg): (bool, Option<String>)) -> Self {
        Verdict { ok, msg }
    }
}

impl From<(bool, String)> for Verdict {
    fn from((ok, msg): (bool, String)) -> Self {
        Verdict { ok, msg: Some(msg) }
    }
}

impl From<f64> for RunOptions {
    fn from(time_limit: f64) -> Self {
        RunOptions {
            time_limit,
            public_wall_time: true,
        }
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            time_limit: 1.0,
            public_wall_time: true,
        }
    }
}

fn fetch_env(name: &str) -> Result<String> {
    env::var(name).map_err(|e| format!("Cannot get environment variable {}: {}", name, e).into())
}

fn get_multiplier(title: &str) -> u64 {
    let tal_meta_dir = env::var("TAL_META_DIR").expect("TAL_META_DIR not set");
    let scores_path = PathBuf::from(tal_meta_dir)
        .parent()
        .unwrap()
        .join("scores.yaml");

    if !scores_path.exists() {
        return 1;
    }

    let scores_content = fs::read_to_string(scores_path).expect("Unable to read scores.yaml");
    let mappings: serde_yaml::Value = serde_yaml::from_str(&scores_content).expect("Invalid YAML");

    let Some(scores) = mappings.get(title) else {
        return 1;
    };
    let Some(score_list) = scores.as_sequence() else {
        return 1;
    };
    for score in score_list {
        if let Some(expiration_date) = score.get("expiration_date") {
            if let Some(expiration_date_str) = expiration_date.as_str() {
                let today = chrono::Local::now().date_naive();
                let expiration_date = NaiveDate::parse_from_str(expiration_date_str, "%Y-%m-%d")
                    .expect("Invalid date format");

                if expiration_date >= today {
                    if let Some(multiplier) = score.get("multiplier") {
                        if let Some(multiplier_value) = multiplier.as_u64() {
                            return multiplier_value;
                        }
                    }
                }
            }
        }
    }

    1
}

pub fn run_tc<I, G, C, T, U, S, V, O>(
    options: O,
    init_fn: I,
    gen_fn: G,
    check_fn: C,
    valid_points: bool,
) -> Result<()>
where
    O: Into<RunOptions>,
    S: IntoIterator<Item = T>,
    V: Into<Verdict>,
    I: FnOnce(Option<&str>) -> Result<S>,
    G: Fn(T) -> Result<U>,
    C: Fn(U) -> Result<V>,
{
    let options = options.into();
    let subtask = fetch_env("TAL_size").ok();
    let output_dir = fetch_env("TAL_META_OUTPUT_FILES")?;
    let mut fout = File::create(format!("{output_dir}/result.txt"))?;
    let mut tc_ok = 0;
    let mut tc_n = 0;
    let iter = init_fn(subtask.as_deref())?.into_iter();
    let total_tc_n = match iter.size_hint() {
        (n, Some(m)) if n == m => n,
        _ => return Err("Cannot get the number of test cases".into()),
    };
    println!("{}", total_tc_n);
    stdout().flush()?;
    for tc_param in iter {
        tc_n += 1;
        let tc = gen_fn(tc_param)?;
        stdout().flush()?;
        let start = Instant::now();
        let verdict = match check_fn(tc) {
            Ok(x) => x.into(),
            Err(e) => {
                writeln!(fout, "Case #{tc_n:03}: RE")?;
                eprintln!("Check error: {}", e);
                continue;
            }
        };
        let elapsed = Instant::now().duration_since(start).as_secs_f64();
        let mut p_verdict = |acr: &str| -> Result<()> {
            use std::fmt::Write;
            let mut verdict = String::new();
            write!(verdict, "Case #{tc_n:03}: {}", acr)?;
            if options.public_wall_time {
                write!(verdict, " | Time: {:.3}s", elapsed)?;
            }
            writeln!(fout, "{}", verdict)?;
            Ok(())
        };
        if elapsed > options.time_limit {
            p_verdict("TLE")?;
        } else if verdict.ok {
            p_verdict("AC")?;
            tc_ok += 1;
        } else {
            p_verdict("WA")?;
        }
        if let Some(msg) = verdict.msg {
            writeln!(fout)?;
            writeln!(fout, "{}", msg)?;
            writeln!(fout)?;
        }
    }
    writeln!(fout)?;
    writeln!(fout, "Score: {}/{}", tc_ok, tc_n)?;
    if valid_points {
        match (
            fetch_env("TAL_META_EXP_TOKEN"),
            fetch_env("TAL_EXT_EXAM_DB"),
        ) {
            (Ok(token), Ok(db_path)) => {
                let conn = Connection::open(db_path)?;
                let problem = fetch_env("TAL_META_CODENAME")?;
                let address = fetch_env("TAL_META_EXP_ADDRESS")?;
                let source = fs::read(format!("{}/source", fetch_env("TAL_META_INPUT_FILES")?))?;

                conn.execute(
                "INSERT INTO submissions (user_id, problem, address, subtime, score, multiplier, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    token,
                    problem,
                    address,
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
                    tc_ok,
                    get_multiplier(&problem),
                    source
                ],
            )?;
            }
            _ => {}
        };
    }
    Ok(())
}

pub fn gen_data<T: Clone>(subtask: &str, data: &[(&str, usize, T)]) -> Vec<T> {
    let mut tc = Vec::new();
    for (name, n, v) in data {
        for _ in 0..*n {
            tc.push(v.clone());
        }
        if subtask == *name {
            break;
        }
    }
    tc
}
