# Logs and reports

Ilium keeps a log of what its server, its client and its plugins do. The diagnostics
plugin shows that log and builds a report you can attach to a bug description.

- The log view lists recent entries, newest last, with their level and source.
- A report is plain text of at most 256 KiB. It holds the version, the platform, the
  enabled plugins and, when you ask for it, the tail of the log.
- Secrets such as API keys are never written into a report.

Read a report before you share it: it can name the folders and projects you worked in.
