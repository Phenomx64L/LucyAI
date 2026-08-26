import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  kit: {
    adapter: adapter({
      fallback: 'index.html', // Esto es vital para que Tauri funcione
      strict: false
    }),
    // Los catálogos CIS se mudaron a `lucy-core/assets/compliance/`, que es de
    // quien son: los lee el motor de cumplimiento en Rust con `include_str!`, y
    // mientras vivieron aquí `lucy-core` no compilaba sin este repositorio al
    // lado — el «corazón sin Tauri» necesitaba la mitad Tauri para existir.
    //
    // El alias evita la otra salida, que era dejar una copia a cada lado. Dos
    // copias de una regla que dice qué comando se ejecuta en la máquina de
    // alguien es la clase de duplicado que se descubre el día que las dos
    // mitades de Lucy discrepan sobre si un equipo cumple.
    //
    // NOMBRE PROPIO Y NO `$lib/compliance`: el `$lib` de SvelteKit es un alias
    // incorporado que se resuelve ANTES, así que un alias más específico
    // colgado de él no llega a mirarse nunca. La build fallaba buscando el
    // fichero en la carpeta de la que se acababa de mover, y el mensaje —«no
    // such file or directory»— no dice que haya un alias siendo ignorado.
    // FUERA DEL REPOSITORIO desde que `lucy-core` salió a ser hermano y no hijo.
    // Los catálogos siguen siendo suyos: los ejecuta el motor de cumplimiento en
    // Rust, y esta aplicación es uno de los dos consumidores, no el dueño.
    alias: {
      $compliance: '../lucy-core/assets/compliance'
    }
  },
  onwarn: (warning, handler) => {
    if (warning.code.startsWith('a11y-')) return;
    handler(warning);
  }
};

export default config;