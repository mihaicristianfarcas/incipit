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

Incipit runs on your computer with your files. It treats three things as
untrusted, because they come from somewhere else:

- **Books.** A PDF or EPUB can be crafted. PDFs are parsed by pdfium, a
  native library; EPUBs are unzipped and their HTML is parsed, never run or
  shown as HTML.
- **What a model writes.** A model's reply is text and data. A figure is a
  typed spec the app checks and draws itself; nothing a model writes is
  executed by the app.
- **Text inside books given to a model.** A book can contain text meant to
  steer a model. It is passed as material, not as instructions.

A problem in any of these (a book that makes the app crash, run code or
read files it should not; a reply that does) is a vulnerability.
