# Agent setup

Agent setup installs small instruction blocks into the configuration of the agent
programs you use, so that an agent started in an Ilium pane knows how to work with Ilium.

- **Install** writes a block into one target, for example the instruction file of one agent.
- **Remove** deletes that block again and leaves the rest of the file as it was.
- **Status** lists every target and says whether its block is installed and current.

Nothing is written without your approval: each change to an agent's configuration is shown
first and applied only when you accept it.

Other plugins can add their own instruction blocks and targets. A block whose plugin is
disabled is no longer offered.
