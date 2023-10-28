<!--
SPDX-FileCopyrightText: 2023 MTRNord

SPDX-License-Identifier: Apache-2.0
-->

# What do I need to know to help?

If you are looking to help to with a code contribution our project uses Rust, Postgres SQL, the IMAP protocol and the SMTP Protocol.
If you don't feel ready to make a code contribution yet, no problem!
You can also check out the [documentation issues](https://github.com/MTRNord/erooster/issues?q=is%3Aopen+is%3Aissue+label%3Adocumentation).

If you are interested in making a code contribution and would like to learn more about the technologies that we use, check out the list below.

- [Rust](https://www.rust-lang.org/)
- [Postgres](https://www.postgresql.org/)
- [IMAP](https://www.rfc-editor.org/rfc/rfc9051.html)
- [SMTP](https://www.rfc-editor.org/rfc/rfc5321.html)

# How do I make a contribution?

Never made an open source contribution before?
Wondering how contributions work in the in our project?
Here's a quick rundown!

1. Find an issue that you are interested in addressing or a feature that you would like to add.
2. Fork the repository associated with the issue to your local GitHub organization.
This means that you will have a copy of the repository under `<your-GitHub-username>/erooster`.
3.Clone the repository to your local machine using `git clone https://github.com/<github-username>/erooster.git`.
4. Create a new branch for your fix using `git checkout -b <your-GitHub-username>/issue-<number-of-the-issue-you-fix>`.
5. Make the appropriate changes for the issue you are trying to address or the feature that you want to add.
6. Use `git add <insert-paths-of-changed-files-here>` to add the file contents of the changed files to the "snapshot" git uses to manage the state of the project, also known as the index.
7. Use `git commit` to store the contents of the index with a descriptive message. Please use the [Conventional Commit](https://www.conventionalcommits.org/en/v1.0.0/) format for your messages.
8. Push the changes to the remote repository using `git push origin branch-name-here`.
9. Submit a pull request to the erooster repository.
10. Title the pull request with a short description of the changes made and the issue or bug number associated with your change. For example, you can title an issue like so "Added more log outputting to resolve #4352".
11. In the description of the pull request, explain the changes that you made, any issues you think exist with the pull request you made, and any questions you have for the maintainer. It's OK if your pull request is not perfect (no pull request is), the reviewer will be able to help you fix any problems and improve it!
12. Wait for the pull request to be reviewed by a maintainer.
13. Make changes to the pull request if the reviewing maintainer recommends them.
14. Celebrate your success after your pull request is merged! 🎉

# Where can I go for help?

If you need help, you can ask questions on [Github Discussions](https://github.com/MTRNord/erooster/discussions) or in our [Matrix Room](https://matrix.to/#/#erooster:nordgedanken.dev).

# What does the Code of Conduct mean for me?

Our Code of Conduct means that you are responsible for treating everyone on the project with respect and courtesy regardless of their identity. If you are the victim of any inappropriate behavior or comments as described in our Code of Conduct, we are here for you and will do the best to ensure that the abuser is reprimanded appropriately, per our code.