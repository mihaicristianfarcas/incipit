# Security

## Reporting a vulnerability

Please report vulnerabilities privately, through GitHub's **private
vulnerability reporting** (the repository's **Security** tab, "Report a
vulnerability"), not in a public issue. Include what an attacker needs (a
crafted book, a model's reply, local access), what they gain, and how to
reproduce it. You will get an answer, and credit in the fix if you want it.

Only the latest code on the default branch is supported. Incipit has not
been externally audited.

## What is trusted

Incipit runs on your computer with your files. It treats four things as
untrusted, because they come from somewhere else:

- **Books.** A PDF or EPUB can be crafted. PDFs are parsed by pdfium, a
  native library; EPUBs are unzipped and their HTML is parsed, never run or
  shown as HTML.
- **What a model writes.** A model's reply is text and data. A figure is a
  typed spec the app checks and draws itself; nothing a model writes is
  executed by the app. A command-line agent runs the shell commands its
  model writes, inside the operating system's sandbox: its own folder, no
  home folder, no network ([docs/models.md](docs/models.md)).
- **Text inside books given to a model.** A book can contain text meant to
  steer a model. It is passed as material, not as instructions.
- **What a later launch sends.** The program running hears later launches on
  a local socket only the same user can reach (a Unix socket only its owner
  may write to, or a named pipe open only to its owner and closed to other
  computers). What arrives is read up to a short limit, anything not of the
  form is dropped, and only paths to PDF and EPUB files that are there are
  taken, as books ([docs/packaging.md](docs/packaging.md#opening-a-book-from-outside)).

A problem in any of these (a book that makes the app crash, run code or
read files it should not; a reply that does) is a vulnerability.
