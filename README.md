<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>UDP Proxy Description</title>
<style>
  body {
    font-family: 'Arial', sans-serif;
    line-height: 1.6;
    background-color: #f4f4f4;
    margin: 0;
    padding: 20px;
  }
  .container {
    max-width: 700px;
    margin: auto;
    background: #fff;
    padding: 20px;
    border-radius: 8px;
    box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
  }
  h1, p {
    color: #333;
  }
  h1 {
    text-align: center;
  }
  .highlight {
    color: #e74c3c;
  }
</style>
</head>
<body>
<div class="container">
  <h1>UDP Game Proxy Overview</h1>
  <p>This <span class="highlight">UDP Proxy</span> acts as an intermediary for game client traffic directed at a multiplayer game server. It's designed to:</p>
  <ul>
    <li>Receive game data from clients on a specified port.</li>
    <li>Forward that data to the game server's port.</li>
    <li>Listen for the game server's responses and forward them back to the correct clients.</li>
  </ul>
  <p>It allows game clients to connect to the game server seamlessly while providing additional benefits like improved security, potential for data inspection, and centralized control over client-server communication.</p>
</div>
</body>
</html>
