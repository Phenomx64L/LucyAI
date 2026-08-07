---
name: ssl-check
description: Revisa un certificado TLS — caducidad, cadena, emisor y qué protocolos acepta el servidor.
---

# Revisión de certificado TLS

Necesitas un host y un puerto. Si el operador no dio el puerto, asume 443 y dilo.

## 1. El certificado

- Windows: `Test-NetConnection <host> -Port 443` primero para saber si siquiera
  hay algo escuchando; luego una conexión TLS que devuelva el sujeto, el emisor y
  las fechas.
- Linux: `echo | openssl s_client -connect <host>:443 -servername <host> 2>/dev/null | openssl x509 -noout -subject -issuer -dates`

**El `-servername` no es opcional.** Sin SNI, un servidor con varios sitios
devuelve el certificado por defecto y el diagnóstico sale mal sin dar ninguna
señal de que salió mal.

## 2. Cuánto le queda

Calcula los días hasta `notAfter`. Por debajo de 30, dilo como aviso; por debajo
de 7, dilo como urgencia. Y comprueba también `notBefore`: un certificado emitido
para dentro de dos días falla igual y confunde el doble.

## 3. La cadena

Un certificado válido con la cadena incompleta funciona en un navegador y falla
en `curl`, en Java y en cualquier cliente que no complete por su cuenta. Mira si
el servidor manda los intermedios.

## 4. Protocolos

Si el operador preguntaba por cumplimiento, comprueba qué versiones acepta.
TLS 1.0 y 1.1 habilitados son un hallazgo que reportar.

## Al terminar

Di la fecha de caducidad **en días**, no solo la fecha: «caduca el 14 de marzo»
obliga a hacer la cuenta, «quedan 9 días» no.
