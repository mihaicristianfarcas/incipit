# Models

Every model call goes through one crate, `incipit-llm`. This page says what
it offers, what each provider needs, how a learner signs in, and what is
done to keep keys and book text in their place.

## The door

A `Provider` answers a `Request` in three ways:

| Call | Gives | Used for |
|---|---|---|
| `text` | plain text, streamed where the provider can | margin answers, the tutor |
| `structured` | JSON checked against a JSON Schema | figures, cards, quizzes, grading |
| `agent` | an agent with a shell, working on files in a folder | changes to the vault |

`structured` asks the provider for JSON (with its own structured output
where it has one), checks the answer against the schema in Incipit, and when
it does not match, sends the problems back once ("/events/2/at: expected
number, found string") and checks the second answer. A second miss is an
error that keeps the answer. The checker knows the keywords structured
output uses (`type`, `properties`, `required`, `additionalProperties`,
`items`, `enum`, `const`, `anyOf`, `oneOf`, `allOf`, local `$ref`, and the
size bounds); others pass, and the engine that reads the value checks its own
sense after.

Calls block. `Call::spawn` runs one on a thread of its own and sends its
updates (text as it arrives, what an agent is doing, a page to open, then the
result) through a channel; `spawn_with` sends them to a callback instead.
Neither needs an async runtime. `Cancel::cancel` stops a call at once: it
kills the child process group or shuts the connection, whatever the call is
waiting on.

## Requests and material

A request has the app's instructions, the material they work on (labelled
pieces of a book, a note, a page), the conversation so far, pictures, and
optionally a model and an effort. Book text and model output are material:
they never become instructions.

- Providers with a system channel get Incipit's preface (`ENGINE`) and the
  instructions there; the material goes into the messages.
- Every piece of material is wrapped in a `<material label="…">` block, and
  `<material` or `</material` inside the text loses its bracket, so a passage
  cannot close its block and speak as the app.
- Earlier answers of the model are material too when a program takes a
  single prompt.
- No provider keeps a session between calls: the conversation is sent with
  each one.

## Command-line agents

Three providers drive a program the learner installed and signed in to
themselves. Incipit never sees their credentials; each program keeps its own
login. A windowed app gets a short `PATH`, so the programs are also looked for
where installers put them (`~/.local/bin`, `/opt/homebrew/bin`, npm's folder
and others), and the child gets those folders in its `PATH` too.

| Provider | Runs | Structured output | Sandbox for agents | Models |
|---|---|---|---|---|
| Codex | `codex exec --json --ephemeral`, the prompt on stdin, the final message from `-o` | `--output-schema` | a permission profile passed with `-c` | `codex debug models`, else Codex's own cache |
| Claude Code | `claude -p --output-format stream-json --no-session-persistence --strict-mcp-config` | `--json-schema`, read from `structured_output` | `--restricted` and sandbox `--settings` | the names `--model` takes (`haiku`, `sonnet`, `opus`, `fable`) |
| OpenCode 2 | `opencode run --standalone --format json` with a config of its own | asked for in the prompt | only its shell, which runs each command under Seatbelt or bubblewrap | `opencode models` |

- **Codex** plain calls run in the read-only sandbox in a scratch folder. A
  turn the service turns away for a moment ("at capacity", rate limits) is
  tried again after 30, 90 and 180 s, moving to the next model the plan lists
  after the first retry. Codex's errors are shown in its own words: the model
  a learner's `config.toml` names may be one a ChatGPT account cannot use,
  and Codex says so.
- **Claude Code** gets Incipit's preface and instructions in
  `--system-prompt` and no tools for plain calls; pictures are copied into
  the call's folder and read with its Read tool. Its text streams.
- **OpenCode** runs on a private server (`--standalone`), not the learner's
  background service, with a config that carries over only their models and
  providers: no plugins, MCP servers, agents or project config. Its session is
  deleted after each call. The effort is the model's variant
  (`provider/model#low`); a model that has none is asked again without it.
  It needs version 2 or later.

Status (installed, signed in, version) comes from `--version` and each
program's login status (`codex login status`, `claude auth status`,
`opencode auth list`), and shows how a program is signed in, never who.

## HTTP providers

The learner can add endpoints in Settings. Each names the protocol it speaks,
its address, and the secret its key is kept under, if it needs one.

| Protocol | Covers | Key | Structured output |
|---|---|---|---|
| OpenAI-compatible (Chat Completions) | OpenAI, OpenRouter, and servers on the learner's own computer such as Ollama and LM Studio | a bearer key, or none for a local server | a strict JSON Schema (`response_format: json_schema`), else JSON mode with the schema in the instructions, else the instructions alone |
| Anthropic (Messages API, `anthropic-version: 2026-10-21`) | Anthropic's API | an API key from the Claude Console (`x-api-key`) | `output_config.format` with the JSON Schema, else the instructions |

Both stream their text as server-sent events. A server that refuses a form
of structured output with a 400 is asked in the next form, and later calls
start there. The answer is checked against the schema in Incipit either way.
An OpenAI-compatible endpoint needs a model chosen in Settings; Anthropic's
falls back to `claude-opus-5-5`. Errors are the service's own words with the
HTTP status, and any key in them is blotted out before they are shown.

## The sandbox

Agents work on files with the shell they already have, inside the operating
system's sandbox: Seatbelt on macOS, bubblewrap (or Landlock, in Codex) on
Linux.

| The agent tries to | Allowed |
|---|---|
| read its folder, and the folders it is given to read | yes |
| write its folder, when it may write | yes |
| write `.git` or `.jj` in its folder, or anywhere else | no |
| read the home folder | no |
| reach the network | no |

OpenCode's agents need the OS sandbox to run commands, so they run on macOS
and Linux only (bubblewrap must be installed there); Codex and Claude Code
bring their own. On Windows no agent has been run.

## Settings

`Settings` say which provider, model and effort do each job (`Quick` for
answers, `Figure` for structured output, `Agent` for the agent) and hold the
HTTP endpoints the learner added. They are plain JSON. An endpoint names the
secret its key is kept under, never the key. `Registry::prepare` picks the
provider for a job and fills in the model and effort the request does not
name itself.

## Keys and tokens

Keys and sign-in tokens are kept only through the `SecretStore` trait.
`FileSecrets` keeps one file per name in a folder of the app's: on unix the
folder is 0700 and each file 0600, written to a new private file that then
replaces the old one. On Windows the files take the ACL of the app's folder
in the user's profile; nothing changes ACLs. The OS keychain can take its
place later behind the same trait. Nothing in the crate logs a key, a token
or an account's address, and the types holding them do not show them in
`Debug`.
