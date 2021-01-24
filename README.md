Jex
===

Jex is an interactive tool for querying json files. It shows a json file in one pane, and the results of a [JQ query](https://stedolan.github.io/jq/manual/) on the right. You can update the query on the fly, allowing you to quickly iterate on your query and find out exactly what you're looking for.

Installing
----------

First, you need to have cargo, the rust package and build manager installed. You can install it by following the instructions at [rustup.rs](https://rustup.rs).

Once you have cargo installed, you can build and install jex by running
```
cargo install jex
```

Use
---

Once you've installed jex, you can use it to open a json file by running `jex example.json`. You can control jex using the following keys:

<!-- START CONTROLS POPUP -->
- Up/down: Scroll through the current pane
- Tab: Switch the active pane
- z: Fold the object or array under the cursor
- q: Open the query editor. Type a JQ query, and press Enter to execute it against the left pane, storing the result in the right pane.
- /: Search
- n: Next search result
- N: Prior search result
- t: Toggle visibility of the edit tree
- j/k: Scroll through the edit tree
- +: Add a new child to the selected view
- r: Rename the current view
- s: Save the current view
- Home: Scroll to the top
- End: Scroll to the bottom
- Esc: Quit jex (or leave the query editor)
- h,? or F1: Show this help text
<!-- END CONTROLS POPUP -->
