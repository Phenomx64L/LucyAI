---
name: dns-troubleshoot
description: Diagnostica problemas de resolución DNS — servidores configurados, consultas, caché y latencia.
---

# Diagnóstico DNS

Cuatro pasos. **No los des todos si uno resuelve el problema**: en cuanto sepas la
causa, dilo y para. Un diagnóstico completo de algo que ya está explicado son
tres comandos que nadie pidió.

## 1. Qué servidores tiene configurados

- Windows: `Get-DnsClientServerAddress -AddressFamily IPv4`
- Linux: `cat /etc/resolv.conf` y, si hay systemd, `resolvectl status`

Fíjate en si son los que se esperan. Un servidor DNS que apunta al router de casa
en una máquina de dominio ya es la respuesta.

## 2. Si resuelve

Prueba un dominio público **y** el que haya mencionado el operador:

- Windows: `Resolve-DnsName google.com` y `Resolve-DnsName <dominio> -Server 8.8.8.8`
- Linux: `dig google.com` y `dig @8.8.8.8 <dominio>`

Comparar contra un DNS público es lo que separa «el dominio no existe» de «este
servidor no lo resuelve».

## 3. La caché

Una entrada envenenada o caducada explica el caso en que unos equipos van y otros
no:

- Windows: `Get-DnsClientCache | Select-Object -First 20`
- Linux: `resolvectl statistics`

Si sospechas de la caché, propón limpiarla — `Clear-DnsClientCache` o
`resolvectl flush-caches`— pero dilo como propuesta: es un cambio, no una lectura.

## 4. Latencia

Solo si lo anterior resuelve bien y la queja era de lentitud. Mide el tiempo de
consulta y compáralo con el DNS público. Por encima de 200 ms hay algo que mirar.

## Al terminar

Di **cuál de los cuatro** dio la respuesta y qué harías. Si nada la dio, dilo
también: «resuelve correctamente desde aquí» es un resultado, y ahorra que el
siguiente busque en el mismo sitio.
