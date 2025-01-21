tmog-events generates a human-readable GitHub activity summary
==============================================================

This is a Rust program that generates a human-readable summary of GitHub activity for a given
user. Instead of using the [GitHub events API](https://docs.github.com/en/rest/activity/events?apiVersion=2022-11-28),
which appears to be limited to very few events, it relies on querying the [Google BigQuery archive
dataset of GitHub events](https://cloud.google.com/blog/topics/public-datasets/github-on-bigquery-analyze-all-the-open-source-code).

This program requires a configuration file which looks like this:

```toml
gcp_project = "testing-123456"
user = "djc"
```

The `gcp_project` is the Google Cloud Platform project that is used to query BigQuery. The `user`
is the GitHub user name whose activity should be summarized.

Additionally, the program takes the period (currently, month) as a CLI argument:

```
Usage: tmog-events [OPTIONS] <MONTH>

Arguments:
  <MONTH>

Options:
      --config <CONFIG>  [default: config.toml]
  -h, --help             Print help
```

The month should be given in the form matching the GitHub archive tables, so `202410` for October 202410.
It's probably possible to make trivial changes to query a different period.

The generated summary is in reStructuredText because that's what I need for my blog at this point,
but changing it to generate Markdown is probably pretty trivial.
