# Models

Every model call goes through one crate, `incipit-llm`. This page says what
it offers, what each provider needs, how a learner signs in, and what is
done to keep keys and book text in their place.

Speaking into a field (voice, an extra) uses none of these: a Whisper model on
the learner's own computer writes down what they say
([architecture.md](architecture.md), "Voice is written down on this computer").

| Provider | The learner needs | Agent |
|---|---|---|
| ChatGPT | a ChatGPT plan, signed in from the app | no |
| Codex | the Codex CLI, signed in with ChatGPT | yes |
| Claude Code | Claude Code, signed in with a Claude account | yes |
| OpenCode | OpenCode 2, connected to an account, or its free models | yes (macOS, Linux) |
| OpenAI-compatible | a key, or a model server on their own computer | no |
| Anthropic | an API key from the Claude Console | no |

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
waiting on. When the app quits, every command-line agent still running is
stopped with what it started (`process::stop_all`), so none goes on working
for an answer no one will read.

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

## Signing in

A learner should be able to use the plan they already pay for without a
terminal. What each company allows a third-party app to do differs.

### ChatGPT

OpenAI documents a way for open-source apps that run on the learner's own
computer to use the learner's ChatGPT plan: "ChatGPT plan usage" in Sign in
with ChatGPT. It is a preview. Incipit does this directly (`chatgpt`):

- The first sign-in registers Incipit for this learner
  (`client_id=dynamic_agent_client`); OpenAI hands back a client id of their
  own (`oaiapp_…`), which later sign-ins reuse. Incipit borrows no other app's
  client id.
- The browser opens OpenAI's authorize page; the answer comes back to a
  listener on `127.0.0.1` and a free port, with fresh `state`, `nonce` and a
  PKCE (S256) challenge. Each computer gets an opaque host id
  (`ext_agent_host_id`, a random UUID).
- The ID token's signature is checked against OpenAI's published keys, then
  its issuer, audience, expiry and nonce. Plan use needs the
  `chatgpt.tokens.use.direct` scope; without it the learner stays signed in
  and is asked to allow it.
- Calls go to the Responses API with `store: false` and `stream: true`, as the
  route requires; only `response.completed` counts as a whole answer. The
  plan's own errors (its usage limit, an account the plan does not cover) are
  said in plain words with OpenAI's code beside them.
- Tokens are refreshed shortly before they expire, one refresh at a time, as
  each refresh token works once. A refresh token that no longer works ends the
  sign-in. Signing out revokes it and keeps the client id and host id.

OpenAI's page for website sign-in says identity-only Sign in with ChatGPT is a
limited trial for selected commercial partners, and the overview sends paid or
remotely hosted apps to an interest form; neither is the route above.

The other way is the Codex CLI's own sign-in. Incipit can start `codex login`
(`cli::login::codex`), pass on the page it prints, and report when it ends,
with no terminal for the learner. The Codex CLI must be installed first.

### Claude

Anthropic does not allow this. Its Claude Code legal page says: "Anthropic
does not permit third-party developers to offer Claude.ai login into their own
applications, or to route requests through Free, Pro, Max, or Ultra plan
credentials on behalf of their users", and that sign-in "must complete
through Anthropic's own flow". The Agent SDK overview says the same, unless
Anthropic approved it beforehand. Incipit has no Claude sign-in of its own.
What remains:

- an API key from the Claude Console, kept in the secret store (the
  Anthropic provider);
- the learner's own Claude Code, installed and signed in by Claude Code's own
  flow. The same page says this does not prevent "an end user from signing in
  to the unmodified Claude Code binary with their own Claude subscription".
  Incipit can start `claude auth login` (`cli::login::claude`) and never reads
  Claude Code's credentials. The page also says the subscription's limits
  assume ordinary, individual use of Claude Code.

### Sources

- OpenAI, ChatGPT plan usage overview:
  <https://developers.openai.com/siwc/token-sharing-open-source>
- Registration and sign-in:
  <https://developers.openai.com/siwc/token-sharing-open-source/sign-in>
- Accounts and sessions (refresh, sign-out):
  <https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions>
- Models and inference:
  <https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference>
- Preview limitations:
  <https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations>
- Errors and recovery:
  <https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery>
- Token reference:
  <https://developers.openai.com/siwc/token-sharing-open-source/token-reference>
- Quickstart: <https://developers.openai.com/siwc/quickstart>
- Website sign-in: <https://developers.openai.com/siwc/website>
- Codex CLI login (its own client id and port 1455):
  <https://github.com/openai/codex/tree/main/codex-rs/login/src>
- Anthropic, Claude Code legal and compliance:
  <https://code.claude.com/docs/en/legal-and-compliance>
- Anthropic, Agent SDK overview:
  <https://code.claude.com/docs/en/agent-sdk/overview>

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

A call can ask to search the web (`Request::search`, and `Abilities::search`
says which providers can). It is for the jobs that look for things outside the
learner's files, such as further reading for a passage. Codex gets
`-c web_search="live"` on its read-only turn; Claude Code gets its `WebSearch`
tool and no other (no `WebFetch`, no file tool); OpenCode's call is allowed
`websearch` and nothing else. The HTTP providers and the ChatGPT sign-in cannot,
and a job that needs the web is not offered with them. What a search finds is
material like any other text, and is shown as links the learner opens
themselves.

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
| write the paths in its folder it is given as read-only | no |
| read the home folder | no |
| reach the network | no |

The vault's agent is given as read-only what the app keeps in its working
copy (`incipit_vault::checks::forbidden_in`: the logs, `.obsidian/`,
`AGENTS.md`, `CLAUDE.md`, `.gitignore`, each book's `book.md` and its own
PDF or EPUB file), so it is refused at once rather than after the run.
The folders it is given to read are the vault's `library` (the books' files
are linked into its working copy from there) and the vault's `.incipit/agent`
(the guide and indexes are, [vault.md](vault.md)); the agent that only suggests
links is given the same, and may write nothing.

Codex gets them as read-only entries in its permission profile; Claude Code
as denied writes in its sandbox settings and denied `Edit` and `Write` rules
for its file tools; OpenCode's shell as paths taken out of the writable
folder in its Seatbelt profile on macOS, and bound read-only by bubblewrap
on Linux, which can only do so for a path that is there when the agent
starts (a log not made yet can still be made). Whatever the sandbox
refused, the files a run changed are checked before it lands
([vault.md](vault.md)).

OpenCode's agents need the OS sandbox to run commands, so they run on macOS
and Linux only (bubblewrap must be installed there); Codex and Claude Code
bring their own. On Windows no agent has been run. A test runs real commands
in OpenCode's sandboxed shell on macOS (reads outside the folder, writes to
`.git` and outside are refused); the Codex and Claude Code settings are the
ones the earlier app checked the same way.

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
