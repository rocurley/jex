Jex
===

Jex is an interactive tool for querying json files. It shows a json file in one pane, and the results of a [JQ query](https://stedolan.github.io/jq/manual/) on the right. You can update the query on the fly, allowing you to quickly iterate on your query and find out exactly what you're looking for.

Installing
----------

First, you need to have cargo, the rust package and build manager installed. You can install it by following the instructions at [rustup.rs](https://rustup.rs).

Once you have cargo installed, you can build and install jex by running
```
cargo install --path .
```
from the root of this repo. Note that the period in the above line is part of the command.

Use
---

Once you've installed jex, you can use it to open a json file by running `jex example.json`. You can control jex using the following keys:

- Up/down: scroll through the current pane
- Tab: switch the active pane
- z: fold the object or array under the cursor
- q: Open the query editor. Type a JQ query, and press Enter to execute it against the left pane, and storing the result in the right pane.
- /: search
- n: next search result
- N: prior search result
- t: toggle visibility of the edit tree
- j/k: scroll through the edit tree
- +: add a new child to the selected view
- r: rename the current view
- Home: go to the top
- End: go to the bottom
- Esc: quit jex (or leave the query editor)
