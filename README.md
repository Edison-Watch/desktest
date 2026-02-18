# Tent - the Testing Agent

### Warning: this is project is in very early alpha, and the agents are very very very dumb with the current screenshotting method

Tent is a command-line tool for testing high-level behaviour for your AppImage desktop applications. Just write a high-level spec in Markdown and let Tent tell you whether it's satisfied.

## Building

You need a vritualisation-capable machine with `docker` and `cargo` installed. Cuttently the app has only been tested on a Debian 13 Trixie minimal server box. Pull the project and run `cargo build`. 

## Usage

You will need an AppImage to use this tool. Either bring your own or use something simple like [elcalc](https://appimage.github.io/elcalc/). You will also need to provide a config, see `config.json.example`. Just setting the key, model, and AppImage location should be sufficient. Finally, you should write your behavioural spec, see `instructions.md.example`.

Once you have all of those up, you can run Tent via cargo like `cargo run -- config.json instructions.md` or by straight up running the binary inside `target/`. There's a `--debug` option as well for more output. While the agent is running there will be a VNC session that you can tune into (by default on `localhost:5900`).

## How it works

Tent makes a minimal container with a virtual X11 server, a VNC server, and everything required to run AppImages for a maximal speedup. It then copies over the AppImage of your choosing to the container and runs it. Once up, an agent is given full control over the VM with the ability to take screenshots, run HCI-equivalent tool calls that are passed onto `xdotool`, and finally call a `done` tool when it decides it has successfully reproduced the instructions given and verified whether the spec is met or not.