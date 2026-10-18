//! Agent-facing help: how to drive an Ilium session from the command line or
//! over HTTP. Plain text so it prints well in a terminal and inside a prompt.

/// One help topic.
pub struct Topic {
    pub name: &'static str,
    pub summary: &'static str,
    pub body: &'static str,
}

/// The default topic printed by `ilium ctl guide`.
pub const ESSENTIALS: &str = "\
ILIUM REMOTE CONTROL - ESSENTIALS

Ilium keeps one tree per session: projects contain groups, groups contain panes
(terminals, agents, editors, boards). Every node has a numeric id. Read ids
first, then act on them.

1. See what exists
   ilium ctl tree                           whole session as JSON
   ilium ctl tree --format markdown         outline for a person or an LLM
   ilium ctl tree --format yaml --depth 2   YAML, two levels deep
   ilium ctl project myproject              one project and everything in it
   ilium ctl pane 12                        full details of one pane
   ilium ctl pane 12 --screen               ...plus its current screen text
   ilium ctl find --agent claude --state idle   panes matching filters
   ilium ctl whoami                         which pane/session am I running in

2. Create things
   ilium ctl new-pane --project myproject                     shell in a project
   ilium ctl new-pane --project myproject --workdir /abs/subdir   shell in a folder
   ilium ctl new-pane --agent claude --prompt 'Fix the failing test'
   ilium ctl new-pane --agent codex --worktree --branch fix-login --prompt 'Fix login'
   ilium ctl new-pane -- cargo test                           run a command
   ilium ctl new-project /abs/path/to/repo
   ilium ctl new-group 'Experiments' --project myproject

3. Control a pane
   ilium ctl send 12 'run the tests'        type text and press Enter
   ilium ctl send 12 --no-enter 'draft'     type without Enter
   ilium ctl keys 12 ctrl-c                 send Ctrl+C (also: enter esc up down tab)
   ilium ctl rename 12 'Login fix'
   ilium ctl close 12                       close the pane
   ilium ctl close 12 --worktree remove     ...and remove its clean, merged worktree
   ilium ctl terminate 12                   stop the process, keep the pane
   ilium ctl schedule 12 --in 600 'status?' send text in 10 minutes
   ilium ctl queue 12 'next step'           send when the agent next finishes

4. Arrange the tree
   ilium ctl move 12 --up                   swap with the previous sibling
   ilium ctl move 12 --to 5 --index 0       make node 5 its parent, first position
   ilium ctl rename 5 'Backend'
   ilium ctl bookmark 12                    pin in the sidebar
   ilium ctl collapse 5 / ilium ctl expand 5
   ilium ctl revert-restructure             undo the last automatic reorganization

5. Wait for results
   ilium ctl wait 12 --state idle,done      block until the agent stops working

Rules of thumb
   - Output is JSON on stdout unless --format says otherwise; errors are JSON
     with \"type\":\"error\" and a non-zero exit status.
   - Target another session with --session NAME, or every session with --all
     on read commands. Inside an Ilium pane the pane's own session is the default.
   - Ids are stable while a pane lives. Re-read the tree before acting on old ids.
   - Anything the CLI can do is also available over HTTP: ilium ctl guide http
   - More: ilium ctl guide topics
";

const TOPICS: &[Topic] = &[
    Topic {
        name: "essentials",
        summary: "The shortest useful tour: read, create, control, arrange.",
        body: ESSENTIALS,
    },
    Topic {
        name: "reading",
        summary: "Tree, project, pane, find and screen commands and their formats.",
        body: "\
READING STATE

ilium ctl sessions                      live sessions on this machine
ilium ctl tree [--format F] [--depth N] [--project P] [--no-details] [--all]
ilium ctl project P [--format F] [--depth N]
ilium ctl pane ID [--format F] [--screen] [--lines N]
ilium ctl node ID [--format F] [--depth N]       any node kind
ilium ctl find [filters]                          flat list of matching panes
ilium ctl screen ID [--lines N]                   current screen text of a terminal pane
ilium ctl whoami                                  the calling pane and its session

Formats: json (default), jsonl (compact), yaml, markdown.
Project selectors P: node id, absolute project path, directory name, or exact
display name. A name that matches two projects is an error listing both ids.
Pane object fields: id, parent, kind, name, short_name, is_bookmarked, and under
\"pane\": content, status{kind, agent, state, goal}, cwd, launch_cwd, project_id,
project_path, last_prompt, scheduled_input, prompt_queue, progress_monitors,
workspace{branch, base_ref, worktree_root}.
Agent states: working, waiting_approval, waiting_subagents, settling, idle, done.
",
    },
    Topic {
        name: "creating",
        summary: "Panes, agents, worktrees, projects, groups, folders.",
        body: "\
CREATING THINGS

ilium ctl new-pane [--project P | --parent ID] [--workdir DIR] [--keep-open]
                   [--agent claude|codex|agy [--prompt TEXT]]
                   [--worktree --branch NAME [--base REF]] [-- CMD...]

  --project P       the project node to place the pane in (id, path, or name).
                    An absolute path that is not yet a project is added first.
  --parent ID       a specific project or group node instead.
  --workdir DIR     start the process in this absolute directory.
  --agent A         start a built-in agent CLI; --prompt is typed once its
                    composer is ready.
  --worktree        create a new Git worktree on --branch (from --base, default
                    the repository's default base) and start the agent in it.
                    Requires --agent; uses the configured worktree location.
  -- CMD...         run a command instead of a shell; the pane closes when it
                    exits unless --keep-open.

The result is JSON with the new pane id. ilium ctl new-project PATH adds a
project; ilium ctl new-group NAME --project P adds a group; ilium ctl
new-folder PATH --parent ID adds a file-browser root.
new-split-view NAME ID ID... [--project P|--parent ID] [--horizontal] groups panes side by side;
ilium ctl op with the mouse operation forwards a mouse event (kind, column, row) to a pane;
replace-command ID CMD swaps the process running in a pane;
new-board NAME DIR [--markdown] [--project P|--parent ID] opens a kanban board;
freeze ID RESUME_CMD keeps a placeholder for a stopped agent, unfreeze ID restarts it.
Voice: ilium voice say SENTENCE... types into the live voice session; HTTP: POST /v1/voice.
Settings: ilium ctl op with the operations set_debug_logging,
set_progress_monitors (enabled true or false), update_sound_settings,
update_detection_settings and update_text_triggers (a full settings object);
ilium ctl ops prints an example of each. They change the running
server only; each client owns the saved copy in ~/.config/ilium/config.toml and
may push it again, so edit that file for a lasting change.
",
    },
    Topic {
        name: "controlling",
        summary: "Send input, keys, scheduled and queued prompts; close, terminate.",
        body: "\
CONTROLLING PANES

ilium ctl send ID TEXT [--no-enter]      type text (Enter by default)
ilium ctl keys ID KEY...                 keys: enter esc tab backspace space up down
                                         left right home end pageup pagedown delete
                                         ctrl-a..ctrl-z text:LITERAL
ilium ctl schedule ID --in SECONDS TEXT [--no-enter]
ilium ctl queue ID TEXT [--times N | --forever]
ilium ctl queue-clear ID
ilium ctl terminate ID                   stop the process, keep the pane
ilium ctl close ID [--worktree keep|remove|remove-and-branch]
ilium ctl wait ID [--state S,...] [--timeout SECONDS]
ilium ctl broadcast ...                  see ilium broadcast --help (all sessions)

Input goes through the server's own prompt-safe paths: text sent to an agent is
submitted as one prompt, not typed key by key. Closing a pane is final.
",
    },
    Topic {
        name: "arranging",
        summary: "Move, reparent, rename, bookmark, expand, lock, restructure.",
        body: "\
ARRANGING THE TREE

ilium ctl move ID --up | --down          swap with a sibling
ilium ctl move ID --to PARENT [--index N]
ilium ctl rename ID TITLE                a rename also stops automatic renaming
ilium ctl bookmark ID [--off]
ilium ctl expand ID / collapse ID
ilium ctl lock-closed ID [--off]
ilium ctl change-folder PROJECT PATH     directory new entries of a project start in
ilium ctl revert-restructure [--project P]  undo the last automatic reorganization
ilium ctl restructure PLAN.json [--project P]  apply a restructure plan (JSON file)

Parents must be projects or groups; panes cannot contain nodes.
",
    },
    Topic {
        name: "http",
        summary: "The same control over loopback HTTP.",
        body: "\
HTTP API (loopback only, default http://127.0.0.1:8872, one server per session)

GET  /v1/guide[?topic=T]                 this help as text/plain
GET  /v1/tree?format=json|yaml|markdown&depth=N&details=0|1
GET  /v1/projects/{selector}?format=...&depth=N
GET  /v1/nodes/{id}?format=...&depth=N
GET  /v1/panes/{id}?format=...&screen=1&lines=N
GET  /v1/panes?agent=claude&state=idle&project=P      flat filtered list
POST /v1/ops                             one operation: {\"op\":\"rename\",\"node\":12,\"title\":\"x\"}
POST /v1/panes                           shorthand for op new_pane
POST /v1/panes/{id}/input                shorthand for op send_text / send_keys
POST /v1/agents                          create an agent (optionally in a new worktree)
POST /create_agent                       the original endpoint, unchanged

Operations (the \"op\" values): rename close move_step reparent new_project
new_group new_folder change_project_folder new_pane send_text send_keys schedule
enqueue clear_queue bookmark expand lock_closed terminate apply_restructure
revert_restructure revert_project_restructure kill_session. Fields match the CLI flags; ilium ctl ops lists them with examples.

Errors: JSON {\"error\": \"...\"} with 400 (bad request or selector), 404 (no such
node), 409 (ambiguous selector), 503 (session shutting down). A successful
operation returns {\"ok\": true, \"op\": ..., \"result\": {...}}.
",
    },
];

/// Every topic, essentials first.
pub fn topics() -> &'static [Topic] {
    TOPICS
}

/// A topic by name, or `None`.
pub fn topic(name: &str) -> Option<&'static Topic> {
    let wanted = name.trim().to_ascii_lowercase();
    TOPICS.iter().find(|topic| topic.name == wanted)
}

/// Text listing the available topics.
pub fn topic_list() -> String {
    let mut text = String::from("Topics (ilium ctl guide TOPIC):\n");
    for topic in TOPICS {
        text.push_str(&format!("  {:<12} {}\n", topic.name, topic.summary));
    }
    text.push_str("  topics       this list\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_resolves_and_is_non_empty() {
        for entry in topics() {
            assert!(!entry.body.trim().is_empty());
            assert_eq!(topic(entry.name).map(|t| t.name), Some(entry.name));
        }
        assert!(topic("nope").is_none());
        assert!(topic_list().contains("essentials"));
    }

    #[test]
    fn essentials_cover_read_create_control_and_arrange() {
        for command in [
            "ilium ctl tree",
            "ilium ctl new-pane",
            "ilium ctl send",
            "ilium ctl move",
            "--worktree",
        ] {
            assert!(ESSENTIALS.contains(command), "{command}");
        }
    }
}
