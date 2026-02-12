Problem:

Automated end-to-end testing of desktop apps kinda sucks because there's a lot of things that can break and it's difficult to define them in the first place in a generic way that's platform agnostic.

Idea:

Run the application that needs to be tested in the most lightweight but fully-featured containerised environment. The user can give some high-level instructions of a feature, how to reproduce it, and what the expected behaviour might be. An LLM takes control of the session with tool calls for common PC interactions (like moving, clicking and typing) and is asked to follow the reproduction steps. It is also allowed (and encouraged) to take screenshots as it goes to make sure it's aware of what's happening. At the end it must write a report of how things behaved, and decide whether it matches or doesn't the expected behaviour.

MVP details:

Let's restrict the scope for the MVP of this project:
 - The app will be a CLI executable, targeting Linux x86_64 machines with containerisation. The container will also be x86_64 for simplicity.
 - It will only take three arguments: the location of a config file, the location of the instructions and whether to run in debug mode
 - Any app being tested will run on x86_64 Linux machines. It will either be an AppImage or a binary inside of a folder which contains all its necessary files.
 - XFCE4 will support all of the features of the app (notifications, tray icon, etc)

Here's how the app will work:
 
 1. The config file is checked for correctness and nothing happens if it's bad
 2. A lightweight x86_64 LXC container is spun up with a logged-in XFCE4 desktop environment with a low-res virtual screen (say 1280x800) in the background. It must have working internet access. A quick check, less than 5 second is acceptable.
 3. A VNC server is spun up inside the container and connection details are printed for easy debugging
 4. The AppImage / executable with resources is copied into the container
 5. The AppImage / executable with resources is run inside the container
 6. Once all of this is up, the main process continues. It sets up an LLM agent based on the key provided in the config. The instructions are some generic "you are a professional software tester experimenting with an app in a Linux Virtual Machine. This is a description of the app you're testing, some things we want you to check, and the expected result. Please report on whether this is correct." followed by instructions for how to interact with the machine, and the contents of the instructions markdown file.
 7. The agent will be allowed to think and call the following tools:
  - moveMouse(posX: int, posY: int) -> None
  - leftClick() -> None
  - rightClick() -> None
  - middleClick() -> None
  - scrollUp(ticks: int) -> None
  - scrollDown(ticks: int) -> None
  - pressAndHoldKey(key: char, milliseconds: int) -> None
  - type(str: string) -> None
  - screenshot() -> Image
  - done(isGood: bool, reasoning: string) -> None
 8. The agentic loop keeps going until the context is exceeded (we might consider compacting in the future) or the `done` tool is called
 9. The result and reasoning is presented to the user. 