use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use clap::Parser;
use gcp_auth::{provider, TokenProvider};
use serde::{Deserialize, Serialize};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut args = Args::parse();
    let provider = provider().await?;
    let config = fs::read(&args.config).context("failed to read config file")?;
    let config = basic_toml::from_slice::<Config>(&config)?;

    let client = reqwest::Client::builder()
        .user_agent(format!(
            "{}@{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;

    args.month.retain(|c| c != '-');
    let cache = format!("{}.json", &args.month);
    let events = match File::open(&cache) {
        Ok(file) => {
            info!(cache, "loading events from cache");
            serde_json::from_reader::<_, Vec<String>>(BufReader::new(file))?
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            info!(cache, "failed to open cache: {err}");
            let events = query(
                &args.month,
                &config.user,
                &config.gcp_project,
                &*provider,
                &client,
            )
            .await?;
            let mut writer = BufWriter::new(File::create(&cache)?);
            info!(cache, "saving events to cache");
            serde_json::to_writer(&mut writer, &events)?;
            events
        }
        Err(err) => return Err(err.into()),
    };

    let mut map = HashMap::<String, HashMap<String, String>>::default();
    for event in events {
        let event = serde_json::from_str::<Event>(&event)?;
        let item = match (event.issue, event.pull_request) {
            (Some(issue), None) => issue,
            (None, Some(pr)) => pr,
            _ => continue,
        };

        let project = match item.project() {
            Some(project) => project,
            None => return Err(anyhow::Error::msg("no project for {item:?}")),
        };

        map.entry(project.to_owned())
            .or_insert_with(HashMap::<String, String>::default)
            .insert(item.html_url, item.title);
    }

    let mut stdout = std::io::stdout().lock();
    for (project, items) in map {
        write!(stdout, "{}\n", project).unwrap();
        for _ in 0..project.len() {
            write!(stdout, "=").unwrap();
        }
        write!(stdout, "\n\n").unwrap();

        let items = items
            .into_iter()
            .map(|(html_url, title)| ItemMeta { title, html_url });
        for ItemMeta { title, html_url } in items {
            write!(stdout, "* `{title} <{html_url}>`_\n")?;
        }

        write!(stdout, "\n").unwrap();
    }

    Ok(())
}

async fn query(
    month: &str,
    user: &str,
    project: &str,
    provider: &dyn TokenProvider,
    client: &reqwest::Client,
) -> anyhow::Result<Vec<String>> {
    info!("requesting token");
    let token = provider
        .token(&["https://www.googleapis.com/auth/bigquery"])
        .await?;

    info!(month, user, project, "querying BigQuery");
    let url = format!("{BIG_QUERY}/projects/{project}/queries");
    let data = JobsQueryData { query: format!("SELECT payload FROM githubarchive.month.{month} WHERE actor.login = '{user}' ORDER BY created_at") };
    let rsp = client
        .post(&url)
        .json(&data)
        .bearer_auth(token.as_str())
        .send()
        .await?;

    let data = rsp.json::<JobsQueryResponse>().await?;
    Ok(data
        .rows
        .into_iter()
        .map(|mut row| {
            assert_eq!(row.f.len(), 1);
            row.f.pop().unwrap().v
        })
        .collect())
}

#[allow(dead_code)] // Helper function for generating formattable JSON
fn dump(path: impl AsRef<Path>, events: &[String]) -> anyhow::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);
    write!(writer, "[\n    ")?;
    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            write!(writer, ",\n    ")?;
        }
        write!(writer, "{}", event)?;
    }
    write!(writer, "\n]\n")?;
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
struct Event {
    issue: Option<ItemMeta>,
    pull_request: Option<ItemMeta>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ItemMeta {
    html_url: String,
    title: String,
}

impl ItemMeta {
    fn project(&self) -> Option<&str> {
        let path = self.html_url.strip_prefix("https://github.com/")?;
        let mut parts = path.splitn(3, '/');
        let org = parts.next()?;
        let repo = parts.next()?;
        Some(match REPO_PROJECT.contains(&org) {
            true => repo,
            false => org,
        })
    }
}

#[derive(Debug, Deserialize)]
struct JobsQueryResponse {
    rows: Vec<Row>,
}

#[derive(Debug, Deserialize)]
struct Row {
    f: Vec<Field>,
}

#[derive(Debug, Deserialize)]
struct Field {
    v: String,
}

#[derive(Debug, Serialize)]
struct JobsQueryData {
    query: String,
}

#[derive(Debug, Parser)]
struct Args {
    month: String,
    #[clap(long, default_value = "config.toml")]
    config: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Config {
    gcp_project: String,
    user: String,
}

const REPO_PROJECT: &[&str] = &["djc", "nicoburns", "seanmonstar", "rust-lang", "hyperium"];
const BIG_QUERY: &str = "https://bigquery.googleapis.com/bigquery/v2";
