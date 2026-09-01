"""Qué recuerda Lucy, y qué cambió desde la última vez que se miró.

PARA QUE UNA PRUEBA DE USO SEA UNA MEDICIÓN Y NO UNA IMPRESIÓN. Casi todo lo que
hace interesante al sistema de memoria es invisible en la pantalla: la confianza
de una fila, cuántas veces se ha recuperado, cuándo caduca, si cruzó el listón de
confirmada, si un patrón llega ya al prompt. Sin esto, «parece que se acuerda» es
todo lo que se puede decir después de una tarde de pruebas — y esa frase no
distingue una memoria que funciona de una que se recupera por casualidad.

Se abre la base en SOLO LECTURA y la foto se guarda FUERA del repositorio, en el
directorio temporal: una herramienta de diagnóstico que deja el árbol sucio en
cada ejecución se deja de usar a la tercera vez.

LOS UMBRALES SE LEEN DE `src/`, no se copian aquí. Es la parte que hace que esto
siga sirviendo dentro de seis meses: un medidor con los números pegados a mano
empieza a mentir en cuanto alguien mueve una constante, y miente en la dirección
peor —enseñando en verde algo que ya no lo está—. Si no encuentra el fuente, lo
dice y sigue con los valores de respaldo en vez de callarse.

USO:
    python tools/memoria.py              foto y diferencia con la anterior
    python tools/memoria.py --limpia     olvida la foto y empieza de cero
    python tools/memoria.py --base RUTA  otra base de datos

    LUCY_DB=C:\\ruta\\lucy.db            lo mismo, por entorno
"""
import io
import json
import os
import re
import sys
import time

import sqlite3

AQUI = os.path.dirname(os.path.abspath(__file__))
FUENTE = os.path.join(AQUI, os.pardir, 'src')
FOTO = os.path.join(
    os.environ.get('TEMP') or os.environ.get('TMPDIR') or '.',
    'lucy-memoria-foto.json',
)

# El respaldo, SOLO para cuando no se encuentra el fuente. Que estos números
# estuvieran aquí como única verdad era el fallo que este bloque evita: la
# primera versión de esta herramienta traía `MIN_MEMORIA = 0.62` copiado de
# memoria, y el valor real era 0,65. Enseñaba un listón que no existía.
RESPALDO = {
    'UMBRAL_CONFIRMADA': 0.80,
    'MIN_CONFIRMADA': 0.58,
    'MIN_MEMORIA': 0.65,
    'VIDA_AUTO_DIAS': 60,
    'FIJADA': 10,
    'MIN_CONFIANZA_PROMPT': 0.60,
    'MAX_EN_PROMPT': 3,
    'REFUERZO': 0.10,
}


def constantes():
    """Los umbrales, leídos del fuente de Rust. Devuelve `(valores, de_dónde)`."""
    v = dict(RESPALDO)
    leidas = 0
    for fichero in ('memories.rs', 'insights.rs'):
        ruta = os.path.join(FUENTE, fichero)
        try:
            txt = io.open(ruta, encoding='utf-8').read()
        except OSError:
            continue
        for nombre in RESPALDO:
            m = re.search(
                r'^pub const ' + nombre + r'\s*:\s*\w+\s*=\s*([0-9_.]+)\s*;',
                txt, re.M)
            if m:
                crudo = m.group(1).replace('_', '')
                v[nombre] = float(crudo) if '.' in crudo else int(crudo)
                leidas += 1
    return v, (f'leídos de src/ ({leidas} constantes)' if leidas
               else 'DE RESPALDO — no encontré src/, pueden estar viejos')


def base():
    for i, a in enumerate(sys.argv):
        if a == '--base' and i + 1 < len(sys.argv):
            return sys.argv[i + 1]
    if os.environ.get('LUCY_DB'):
        return os.environ['LUCY_DB']
    return os.path.join(os.environ.get('APPDATA', ''), 'com.lucy.dev', 'lucy.db')


def corta(s, n=58):
    s = ' '.join((s or '').split())
    return s if len(s) <= n else s[:n - 1] + '…'


def cuando(ts):
    if not ts:
        return '—'
    d = ts - time.time()
    return 'CADUCADA' if d < 0 else f'en {int(d // 86400)} d'


def lee(ruta):
    con = sqlite3.connect(f'file:{ruta}?mode=ro', uri=True)
    con.row_factory = sqlite3.Row
    est = {'memorias': {}, 'insights': {}, 'audit': {}, 'gasto': {}}

    # Las mismas dos exclusiones que usa el núcleo para «memorias vivas»: lo
    # supersedido por la consolidación ya no es verdad, y los trozos de PDF no
    # son memorias de esta instalación. Contarlos aquí daría un total que no
    # cuadra con lo que enseña la aplicación.
    for r in con.execute("""
        SELECT id, title, confidence, access_count, expires_at, tags,
               importance, pinned
        FROM agent_memories
        WHERE (superseded_by IS NULL OR superseded_by = '')
          AND session_id NOT LIKE 'pdf:%'
        ORDER BY id DESC LIMIT 500
    """):
        est['memorias'][str(r['id'])] = {
            'titulo': r['title'],
            'conf': round(r['confidence'] or 0.0, 4),
            'accesos': r['access_count'] or 0,
            'caduca': r['expires_at'] or 0,
            'tags': r['tags'] or '',
            'imp': r['importance'] or 0,
            'fijada': bool(r['pinned']),
        }

    try:
        for r in con.execute('SELECT id, content, confidence, reinforcements, '
                             'rejected_at FROM agent_insights'):
            est['insights'][str(r['id'])] = {
                'texto': r['content'],
                'conf': round(r['confidence'] or 0.0, 4),
                'refuerzos': r['reinforcements'] or 0,
                'descartado': bool(r['rejected_at'] or 0),
            }
    except sqlite3.Error:
        # `rejected_at` es de la lápida y no existe en una base anterior.
        for r in con.execute('SELECT id, content, confidence, reinforcements '
                             'FROM agent_insights'):
            est['insights'][str(r['id'])] = {
                'texto': r['content'],
                'conf': round(r['confidence'] or 0.0, 4),
                'refuerzos': r['reinforcements'] or 0,
                'descartado': False,
            }

    for t, destino, extra in (
        ('audit_trail', 'audit', 'source'),
        ('token_usage', 'gasto', 'request_type'),
    ):
        try:
            if destino == 'audit':
                for r in con.execute(
                        f'SELECT {extra} k, COUNT(*) n FROM {t} GROUP BY {extra}'):
                    est['audit'][r['k']] = r['n']
            else:
                for r in con.execute(
                        f'SELECT {extra} k, COUNT(*) n, COALESCE(SUM(total_cost),0) c '
                        f"FROM {t} WHERE created_at > datetime('now','-1 day') "
                        f'GROUP BY {extra}'):
                    est['gasto'][r['k']] = [r['n'], round(r['c'], 5)]
        except sqlite3.Error:
            pass

    con.close()
    return est


def pinta(est, k):
    m = est['memorias']
    conf = [v for v in m.values() if v['conf'] >= k['UMBRAL_CONFIRMADA']]
    print(f'{len(m)} memorias vivas · {len(conf)} confirmadas '
          f'(>= {k["UMBRAL_CONFIRMADA"]}) · '
          f'{len([v for v in m.values() if v["caduca"]])} con plazo')
    tocadas = sorted((v for v in m.values() if v['accesos']),
                     key=lambda v: -v['accesos'])[:8]
    if tocadas:
        print('  las más recuperadas:')
        for v in tocadas:
            marca = ' [confirmada]' if v['conf'] >= k['UMBRAL_CONFIRMADA'] else ''
            print(f'    {v["accesos"]:>3} accesos  conf {v["conf"]:.3f}{marca}  '
                  f'{corta(v["titulo"])}')
    else:
        print('  ninguna se ha recuperado nunca (access_count = 0 en todas)')

    ins = [v for v in est['insights'].values() if not v['descartado']]
    tope = k['MIN_CONFIANZA_PROMPT']
    entran = [v for v in ins if v['conf'] >= tope]
    fuera = len(est['insights']) - len(ins)
    print(f'{len(ins)} patrones vivos · {len(entran)} pasan el listón del prompt '
          f'({tope}) · {fuera} descartados')
    for v in sorted(ins, key=lambda v: -v['conf'])[:5]:
        print(f'    conf {v["conf"]:.3f}  x{v["refuerzos"]}  {corta(v["texto"], 48)}'
              f'{"  <- en el prompt" if v["conf"] >= tope else ""}')
    if ins and not entran:
        faltan = falta_para(max(v['conf'] for v in ins), tope, k['REFUERZO'])
        print(f'    (al mejor le faltan {faltan} refuerzos para entrar)')

    if est['audit']:
        print('auditoría por origen: '
              + ' · '.join(f'{a} {n}' for a, n in sorted(est['audit'].items())))
    if est['gasto']:
        print('gasto de las últimas 24 h: '
              + ' · '.join(f'{a} {v[0]} llamadas ${v[1]}'
                           for a, v in sorted(est['gasto'].items())))


def falta_para(desde, hasta, paso):
    """Cuántos refuerzos hacen falta. La misma curva que `insights::guarda`."""
    n, c = 0, desde
    while c < hasta and n < 99:
        c += paso * (1.0 - c)
        n += 1
    return n


def compara(viejo, nuevo, k):
    print('\n' + '=' * 68)
    print('QUÉ HA CAMBIADO DESDE LA FOTO ANTERIOR')
    print('=' * 68)
    hubo = False
    vm, nm = viejo['memorias'], nuevo['memorias']

    for a in nm:
        if a not in vm:
            v, hubo = nm[a], True
            print(f'+ MEMORIA NUEVA  id={a}  conf {v["conf"]:.3f}  '
                  f'caduca {cuando(v["caduca"])}  tags={v["tags"]}')
            print(f'    {corta(v["titulo"], 70)}')
    for a in vm:
        if a not in nm:
            hubo = True
            print(f'- MEMORIA FUERA  id={a}  {corta(vm[a]["titulo"], 60)}')
            print('    (supersedida por la consolidación, o borrada)')
    for a in nm:
        if a not in vm:
            continue
        x, y = vm[a], nm[a]
        cambios = []
        if abs(x['conf'] - y['conf']) > 1e-9:
            cruce = ('  << CRUZA EL UMBRAL DE CONFIRMADA'
                     if x['conf'] < k['UMBRAL_CONFIRMADA'] <= y['conf'] else '')
            cambios.append(f'confianza {x["conf"]:.3f} → {y["conf"]:.3f}{cruce}')
        if x['accesos'] != y['accesos']:
            cambios.append(f'accesos {x["accesos"]} → {y["accesos"]}')
        if x['caduca'] != y['caduca']:
            if y['caduca'] == 0:
                cambios.append('YA NO CADUCA (se le quitó el plazo)')
            elif x['caduca'] == 0:
                cambios.append(f'ahora caduca {cuando(y["caduca"])}')
            else:
                cambios.append(
                    f'plazo corrido {(y["caduca"] - x["caduca"]) / 86400:+.1f} días')
        if x['fijada'] != y['fijada']:
            cambios.append('fijada' if y['fijada'] else 'sin fijar')
        if cambios:
            hubo = True
            print(f'~ id={a}  {corta(y["titulo"], 60)}')
            for c in cambios:
                print(f'    {c}')

    vi, ni = viejo['insights'], nuevo['insights']
    for a in ni:
        if a not in vi:
            hubo = True
            print(f'+ PATRÓN NUEVO  conf {ni[a]["conf"]:.3f}  '
                  f'{corta(ni[a]["texto"], 58)}')
            continue
        x, y = vi[a], ni[a]
        if x['descartado'] != y['descartado']:
            hubo = True
            print(f'~ PATRÓN DESCARTADO — ya no puede volver  {corta(y["texto"], 45)}')
        elif abs(x['conf'] - y['conf']) > 1e-9:
            hubo = True
            cruce = ('  << YA ENTRA EN EL PROMPT'
                     if x['conf'] < k['MIN_CONFIANZA_PROMPT'] <= y['conf'] else '')
            print(f'~ PATRÓN  conf {x["conf"]:.3f} → {y["conf"]:.3f}{cruce}')
            print(f'    {corta(y["texto"], 60)}')

    for a in sorted(set(viejo['audit']) | set(nuevo['audit'])):
        x, y = viejo['audit'].get(a, 0), nuevo['audit'].get(a, 0)
        if x != y:
            hubo = True
            print(f'~ AUDITORÍA «{a}»  {x} → {y} filas')

    if not hubo:
        print('nada. Ni una memoria nueva, ni un acceso, ni un cambio de confianza.')
    print('=' * 68)


def main():
    if '--limpia' in sys.argv:
        if os.path.exists(FOTO):
            os.remove(FOTO)
        print(f'foto anterior borrada ({FOTO}).')
        return
    ruta = base()
    if not os.path.exists(ruta):
        print(f'no encuentro la base en {ruta}')
        print('usa --base RUTA o la variable LUCY_DB.')
        return
    k, de_donde = constantes()
    nuevo = lee(ruta)
    print(f'--- {time.strftime("%H:%M:%S")} · {ruta}')
    print(f'--- umbrales: {de_donde}')
    pinta(nuevo, k)
    if os.path.exists(FOTO):
        with io.open(FOTO, encoding='utf-8') as f:
            compara(json.load(f), nuevo, k)
    else:
        print('\n(primera foto: no hay con qué comparar. Vuelve a correrlo '
              'después de trabajar con Lucy.)')
    with io.open(FOTO, 'w', encoding='utf-8') as f:
        json.dump(nuevo, f, ensure_ascii=False)


main()
