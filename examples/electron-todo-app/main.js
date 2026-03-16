const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const fs = require('fs');

const todosPath = path.join(app.getPath('userData'), 'todos.json');

function createWindow() {
  const win = new BrowserWindow({
    width: 800,
    height: 600,
    webPreferences: {
      nodeIntegration: true,       // NOTE: insecure; acceptable only in isolated test containers
      contextIsolation: false,     // NOTE: insecure; acceptable only in isolated test containers
    },
  });

  win.loadFile('index.html');
}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
  app.quit();
});

// Handle saving todos
ipcMain.on('save-todos', (event, todos) => {
  fs.writeFileSync(todosPath, JSON.stringify(todos, null, 2));
});
