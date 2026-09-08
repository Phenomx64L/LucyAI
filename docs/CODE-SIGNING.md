# Firma de código — qué certificado hace falta y por qué

Windows no ejecuta binarios desconocidos sin firmar. Esto dejó de ser un
problema teórico el 2026-08-04: Smart App Control bloqueó en la máquina de
desarrollo el ejecutable de tests de `lucy-core` recién enlazado
(`os error 4551`), y antes había bloqueado la primera compilación del shell
nativo. El mismo muro espera a cualquier usuario que descargue el instalador.

Este documento existe para que la decisión de compra se tome con los datos
correctos. **No todos los certificados de firma sirven para lo mismo.**

## Los tres niveles, y qué consigue cada uno

| | Auto-firmado | OV (validación de organización) | EV (validación extendida) |
|---|---|---|---|
| Coste | 0 | ~100-300 USD/año | ~300-500 USD/año |
| Validación | ninguna | documental, días | documental + presencial, más días |
| Almacenamiento de la clave | fichero | fichero o token | **token físico o HSM en la nube, obligatorio** |
| ¿Quita "Editor desconocido"? | no | sí | sí |
| Reputación en SmartScreen | ninguna | **se construye con descargas** | **inmediata** |
| Smart App Control | no basta | puede seguir bloqueando al principio | es lo que tiene opción de pasar |

Las dos filas que deciden:

- **El auto-firmado no sirve para esto.** Quita el aviso solo en máquinas donde
  el certificado se haya instalado a mano en el almacén de confianza. Para un
  usuario que descarga el instalador no cambia absolutamente nada. Es útil para
  probar la *mecánica* de firma, no para distribuir.

- **OV frente a EV es una cuestión de tiempo.** Con OV, los primeros usuarios
  siguen viendo el aviso de SmartScreen hasta que el binario acumula descargas
  suficientes — y ese contador se reinicia con cada versión nueva, aunque el
  certificado sea el mismo. Con EV, la reputación es inmediata desde la primera
  descarga. Para un producto que publica versiones a menudo, OV significa que
  casi ninguna versión llega a tener reputación antes de ser sustituida.

Para Lucy, que es una herramienta de administración de sistemas y va a pedir
privilegios, la recomendación es **EV**. Un aviso de "editor desconocido" en una
aplicación que ejecuta PowerShell con permisos es la peor combinación posible: el
usuario que sí se preocupa la rechaza, y el que no, se acostumbra a ignorar
avisos.

## Lo que NO arregla firmar

**El bucle de desarrollo.** `cargo test` reenlaza el binario de tests con un hash
nuevo en cada cambio, y `cargo build` compila decenas de DLL de macros dentro de
`target/`. Firmar todo eso exigiría un paso de firma después de cada compilación
— y con EV, cada firma pasa por el token físico. Para desarrollar en una máquina
con Smart App Control activo la salida es compilar en una máquina sin esa
política.

Aquí ponía además «verificar en CI, que es lo que ya hace este repositorio en
`windows-latest`». **No lo hace.** Los flujos de trabajo se fueron del árbol con
la V1 en septiembre de 2026 y no se han repuesto; `.github/` solo tiene
`CODEOWNERS` y `FUNDING.yml`. Queda escrito porque un documento que da por
existente una red que no está es peor que uno que no la menciona.

## Cómo queda cableado cuando haya certificado

Hay UN camino, y antes había dos. El empaquetador de Tauri firmaba el bundle a
partir de `src-tauri/tauri.conf.json`; con la V1 fuera del árbol eso ya no
existe, y lo que queda es el paso explícito que el shell nativo siempre necesitó.

### El ejecutable

Tras compilar, con el certificado **ya importado en el almacén de Windows** de la
máquina que firma:

```
signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /sha1 <huella> lucy-egui.exe
```

**El sello de tiempo no es opcional en la práctica.** Sin `/tr`, la firma deja de
ser válida el día que el certificado caduca, y todo lo que se distribuyó antes se
convierte en no firmado. Con sello sigue siendo válida para siempre, porque
acredita que se firmó mientras el certificado estaba vigente. Es el parámetro que
más caro sale olvidar y el que no da ningún síntoma hasta años después.

### El instalador y los MSI

Se firman igual y **después** del ejecutable: NSIS y WiX empaquetan el binario
dentro, así que firmar el instalador primero y el ejecutable después dejaría
dentro del paquete una copia sin firmar. El orden es compilar, firmar el `.exe`,
empaquetar, firmar el `.exe` del instalador y cada `.msi`.

`packaging/build-all.ps1` no tiene hoy ningún paso de firma. Añadirlo es el sitio
correcto: es el único punto por el que pasan las seis salidas.

## Antes de comprar, comprobar

1. Que el emisor entrega el certificado en el formato que acepta el flujo
   elegido (token físico para EV; fichero o token para OV).
2. Que la identidad que se valida es la misma que figura en `bundle.publisher`
   — hoy `Iván Eduardo Luna (@Phenomx64L)`. Si el certificado sale a nombre de
   una entidad distinta, el instalador dirá un nombre y la firma otro.
3. Que el CI puede acceder a la clave. Con EV en token físico, un runner en la
   nube **no puede** firmar: hace falta un servicio de firma en HSM (Azure Key
   Vault, DigiCert KeyLocker o equivalente) o firmar desde una máquina propia.
   Esto es lo que más veces se descubre tarde.
