const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');
const fs = require('fs');

const todosPath = path.join(app.getPath('userData'), 'todos.json');

function createWindow() {
  const win = new BrowserWindow({
    width: 800,
    height: 600,
    show: false,
    backgroundColor: '#ffffff',
    webPreferences: {
      nodeIntegration: true,       // NOTE: insecure; acceptable only in isolated test containers
      contextIsolation: false,     // NOTE: insecure; acceptable only in isolated test containers
    },
  });

  win.loadFile('index.html');

  // Show window after content is ready — ensures the Chromium compositor
  // has finished initializing, which is required in VM environments (Tart)
  // where the default show-on-create path can produce invisible windows.
  win.once('ready-to-show', () => {
    win.show();
    win.focus();
  });
}

app.whenReady().then(createWindow);

app.on('window-all-closed', () => {
  app.quit();
});

// Handle saving todos
ipcMain.on('save-todos', (event, todos) => {
  fs.writeFileSync(todosPath, JSON.stringify(todos, null, 2));
});
