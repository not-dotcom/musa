// Preload mínimo: no expone Node al contenido remoto. La ventana solo se usa
// para que MusicKit/Widevine reproduzcan; el control se hace con scripts
// aislados desde main.js.
'use strict';

const { contextBridge } = require('electron');

contextBridge.exposeInMainWorld('musaSidecar', {
  version: 1,
});
