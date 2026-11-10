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
