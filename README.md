Jed
===

Jed is an interactive tool for querying json files. It shows a json file in one pane, and the results of a [JQ query](https://stedolan.github.io/jq/manual/) on the right. You can update the query on the fly, allowing you to quickly iterate on your query and find out exactly what you're looking for.

Installing
----------

First, you need to have cargo, the rust package and build manager installed. You can install it by following the instructions at [rustup.rs](https://rustup.rs).

Once you have cargo installed, you can build and install jed by running `cargo install --path .`

Use
---

Once you've installed jed, you can use it to open a json file by running `jed example.json`. You can control jed using the following keys:

- Up/down: sroll through the current pane
- Tab: switch the activer pane
- z: fold the object or array under the cursor
- q: Open the query editor. Type a JQ query, and press Enter to execute it against the left pane, and storing the result in the right pane.
- Esc: Quit jed (or leave the query editor)
