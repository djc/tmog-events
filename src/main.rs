use std::collections::HashMap;
use std::fs;
use std::io::Write;

use chrono::{DateTime, Datelike, Months, Utc};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, LINK};
use serde::Deserialize;

#[tokio::main]
async fn main() {
    let prev = Utc::now().checked_sub_months(Months::new(1)).unwrap();
    let start = (prev.year(), prev.month());
    let end = (prev.year(), prev.month() + 1);

    let mut headers = HeaderMap::<HeaderValue>::default();
    if let Ok(token) = fs::read_to_string("token.txt") {
        eprintln!("using token from token.txt");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", token.trim())).unwrap(),
        );
    };

    let client = reqwest::Client::builder()
        .user_agent(format!(
            "{}@{}",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION")
        ))
        .default_headers(headers)
        .build()
        .unwrap();

    let mut map = HashMap::<String, HashMap<String, String>>::default();
    let mut cur = Some(URL.to_owned());
    'outer: loop {
        let url = match cur.take() {
            Some(url) => url,
            None => break,
        };

        eprintln!("fetching {url}");
        let rsp = client.get(url).send().await.unwrap();
        let link = rsp.headers().get(LINK).and_then(|hv| hv.to_str().ok());
        let link = match link {
            Some(link) => link,
            None => {
                eprintln!("{}", rsp.text().await.unwrap());
                break;
            }
        };

        for link in link.split(", ") {
            if let Some((url, rel)) = link.split_once("; ") {
                if rel == "rel=\"next\"" {
                    cur = Some(
                        url.strip_prefix('<')
                            .unwrap()
                            .strip_suffix('>')
                            .unwrap()
                            .to_owned(),
                    );
                }
            }
        }

        let events = rsp.json::<Vec<Event>>().await.unwrap();
        for event in events {
            let mut data = match event {
                Event::IssueComment(meta) => EventData::new(meta),
                Event::Issues(meta) => EventData::new(meta),
                Event::PullRequest(meta) => EventData::new(meta),
                Event::PullRequestReview(meta) => EventData::new(meta),
                Event::PullRequestReviewComment(meta) => EventData::new(meta),
                Event::Release(meta) => EventData::new(meta),
                _ => continue,
            };

            let month = (data.dt.year(), data.dt.month());
            if month >= end {
                continue;
            } else if month < start {
                break 'outer;
            }

            if data.url.contains("/issues/") && data.node_id.starts_with("PR_") {
                data.url = data.url.replace("/issues/", "/pull/");
            } else if data.url.contains("/pulls/") {
                data.url = data.url.replace("/pulls/", "/pull/");
            }

            map.entry(data.project.clone())
                .or_insert_with(HashMap::default)
                .insert(data.url, data.title);
        }
    }

    let mut stdout = std::io::stdout().lock();
    for (project, items) in map {
        write!(stdout, "{}\n", project).unwrap();
        for _ in 0..project.len() {
            write!(stdout, "=").unwrap();
        }
        write!(stdout, "\n\n").unwrap();

        for (url, title) in items {
            let path = match url.strip_prefix(PREFIX) {
                Some(path) => path,
                None if url.starts_with("https://github.com/") => &url,
                None => {
                    eprintln!("unexpected url: {url}");
                    continue;
                }
            };
            write!(stdout, "* `{title} <https://github.com{path}>`_\n").unwrap();
        }

        write!(stdout, "\n").unwrap();
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "CreateEvent")]
    Create,
    #[serde(rename = "DeleteEvent")]
    Delete,
    #[serde(rename = "ForkEvent")]
    Fork,
    #[serde(rename = "IssueCommentEvent")]
    IssueComment(EventMeta<IssueEvent>),
    #[serde(rename = "IssuesEvent")]
    Issues(EventMeta<IssueEvent>),
    #[serde(rename = "MemberEvent")]
    Member,
    #[serde(rename = "PublicEvent")]
    Public,
    #[serde(rename = "PullRequestEvent")]
    PullRequest(EventMeta<PullRequestEvent>),
    #[serde(rename = "PullRequestReviewEvent")]
    PullRequestReview(EventMeta<PullRequestEvent>),
    #[serde(rename = "PullRequestReviewCommentEvent")]
    PullRequestReviewComment(EventMeta<PullRequestEvent>),
    #[serde(rename = "PushEvent")]
    Push,
    #[serde(rename = "ReleaseEvent")]
    Release(EventMeta<ReleaseEvent>),
    #[serde(rename = "WatchEvent")]
    Watch,
}

#[derive(Debug, Deserialize)]
struct EventMeta<T> {
    repo: Repo,
    created_at: DateTime<Utc>,
    payload: T,
}

struct EventData {
    project: String,
    dt: DateTime<Utc>,
    node_id: String,
    url: String,
    title: String,
}

impl EventData {
    fn new(meta: EventMeta<impl Into<ItemMeta>>) -> Self {
        let item = meta.payload.into();
        Self {
            project: project(meta.repo.name),
            dt: meta.created_at,
            node_id: item.node_id,
            url: item.url,
            title: item.title,
        }
    }
}

#[derive(Debug, Deserialize)]
struct IssueEvent {
    issue: ItemMeta,
}

impl Into<ItemMeta> for IssueEvent {
    fn into(self) -> ItemMeta {
        self.issue
    }
}

#[derive(Debug, Deserialize)]
struct PullRequestEvent {
    pull_request: ItemMeta,
}

impl Into<ItemMeta> for PullRequestEvent {
    fn into(self) -> ItemMeta {
        self.pull_request
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseEvent {
    release: ReleaseData,
}

impl Into<ItemMeta> for ReleaseEvent {
    fn into(self) -> ItemMeta {
        ItemMeta {
            node_id: self.release.node_id,
            url: self.release.html_url,
            title: self.release.name,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ReleaseData {
    node_id: String,
    html_url: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ItemMeta {
    node_id: String,
    url: String,
    title: String,
}

#[derive(Debug, Deserialize)]
struct Repo {
    name: String,
}

fn project(project: String) -> String {
    let (scope, repo) = project.split_once('/').unwrap();
    match PEOPLE.contains(&scope) {
        true => repo.to_owned(),
        false => project.to_owned(),
    }
}

const PEOPLE: &[&str] = &["djc", "nicoburns", "seanmonstar"];
const PREFIX: &str = "https://api.github.com/repos";
//const URL: &str = "https://api.github.com/users/djc/events/public?per_page=100";
const URL: &str = "https://api.github.com/events?per_page=100";
