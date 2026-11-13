'use strict';

// Utilidades puras del protocolo del sidecar, separadas para poder probarlas
// con Node sin arrancar Electron.

function ok(id, data) {
  return { v: 1, id, ok: true, data };
}

function fail(id, code, message) {
  return { v: 1, id, ok: false, error: { code, message } };
}

const COMMANDS = new Set([
  'hello',
  'show-login-window',
  'play',
  'pause',
  'seek',
  'volume',
  'state',
  'session',
  'shutdown',
]);

function isCommand(value) {
  return COMMANDS.has(value);
}

module.exports = { ok, fail, isCommand, COMMANDS };
