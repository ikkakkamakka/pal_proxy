# UDP Proxy Overview 🌐

This **UDP Proxy** acts as an intermediary for game client traffic directed at a multiplayer game server. In my case, I wanted to create something that would allow me to turn on the server automatically when a client tries to connect to the game server.

![image](https://github.com/ikkakkamakka/pal_proxy/assets/66095129/8f1e5ddd-44d7-4443-8749-8e3492a50f7b)


- 📥 Receive game data from clients on a specified port.
- 🔁 Forward that data to the game server's port.
- 📤 Listen for the game server's responses and forward them back to the correct clients.

It allows game clients to connect to the game server seamlessly while providing additional benefits like:

- **Security** 🔐: Adds a layer of security between clients and the server.
- **Inspection** 🕵️‍♂️: Can inspect and log the data for monitoring or debugging purposes.
- **Control** 🎮: Centralized control over client-server communication.

## Features

| Feature      | Description |
|--------------|-------------|
| Fast         | ⚡ High-performance proxy handling. |
| Lightweight  | 🪶 Minimal resource usage. |
| Customizable | 🛠 Configurable for different environments. |

<!-- Example of using HTML for a badge-like element -->
<span style="color: green;">●</span> **Running**
