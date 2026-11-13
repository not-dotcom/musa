'use strict';

// MusicKit resuelve la cola de forma atómica: si un solo id no existe en el
// storefront (por ejemplo, una canción de la biblioteca cuyo id de catálogo
// Apple ya sustituyó), rechaza la lista entera con `[mk-007] NOT_FOUND`. En vez
// de perder la reproducción completa, quitamos esos ids y reintentamos.
//
// Estas funciones son puras (reciben `setQueue` como callback) para poder
// probarlas con Node sin arrancar Electron.

const MAX_ATTEMPTS = 6;

/** Ids pedidos que MusicKit menciona como no resueltos en el error. */
function unresolvedIds(error, requested) {
  const message = String((error && error.message) || error || '');
  if (!message) return [];
  const found = requested.filter((id) => message.includes(String(id)));
  if (found.length) return found;
  // Formatos alternativos: "could not be resolved: 123, 456" o "not found: 123".
  const tail = /(?:resolved|found)\s*:\s*(.+)$/i.exec(message);
  if (!tail) return [];
  const tokens = tail[1]
    .split(/[\s,;]+/)
    .map((token) => token.replace(/[\[\]()'"]/g, '').trim())
    .filter(Boolean);
  return requested.filter((id) => tokens.includes(String(id)));
}

/**
 * Devuelve la cola sin los ids no disponibles y el índice equivalente para que
 * siga sonando la misma canción (o la siguiente superviviente).
 */
function queueAfterRemoving(ids, startIndex, missing) {
  const gone = new Set(missing.map(String));
  const kept = ids.filter((id) => !gone.has(String(id)));
  const start = Math.max(0, Math.min(Number(startIndex) || 0, Math.max(0, ids.length - 1)));
  const startId = ids[start];
  let index;
  if (startId !== undefined && !gone.has(String(startId))) {
    index = kept.indexOf(startId);
  } else {
    let before = 0;
    for (let i = 0; i < start && i < ids.length; i++) {
      if (!gone.has(String(ids[i]))) before++;
    }
    index = before;
  }
  return { ids: kept, index: Math.max(0, Math.min(index, kept.length - 1)) };
}

/**
 * Establece la cola en MusicKit y, si algún id ya no se resuelve, reintenta sin
 * él. `setQueue` recibe `{song_ids, index, play}` y debe rechazar con el error
 * de MusicKit si la lista completa no es válida.
 *
 * Devuelve `{queue, index, dropped}` con lo que realmente quedó en reproducción.
 */
async function setQueueResilient(setQueue, params = {}) {
  let ids = (Array.isArray(params.song_ids) ? params.song_ids : []).map(String);
  if (!ids.length) throw new Error('la cola está vacía');
  let index = Math.max(0, Math.min(Number(params.index) || 0, ids.length - 1));
  const dropped = [];
  let lastError = null;

  for (let attempt = 0; attempt < MAX_ATTEMPTS && ids.length; attempt++) {
    try {
      await setQueue({ song_ids: ids, index, play: params.play });
      return { queue: ids, index, dropped };
    } catch (error) {
      const missing = unresolvedIds(error, ids);
      if (!missing.length) throw error;
      const next = queueAfterRemoving(ids, index, missing);
      if (!next.ids.length) {
        throw new Error('ninguna canción de la lista está disponible en Apple Music');
      }
      dropped.push(...missing);
      lastError = error;
      ids = next.ids;
      index = next.index;
    }
  }
  if (lastError) throw lastError;
  return { queue: ids, index, dropped };
}

module.exports = { unresolvedIds, queueAfterRemoving, setQueueResilient };
