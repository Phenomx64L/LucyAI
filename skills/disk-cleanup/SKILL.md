---
name: disk-cleanup
description: Encuentra qué está llenando un disco y propone qué liberar, sin borrar nada por su cuenta.
---

# Espacio en disco

**Este skill no borra.** Encuentra, mide y propone; borrar lo aprueba el operador
viendo qué se va. Un procedimiento automático que libera espacio es un
procedimiento que un día se lleva algo que hacía falta.

## 1. Cuánto falta y dónde

- Windows: `Get-Volume | Where-Object DriveLetter | Select-Object DriveLetter, @{n='LibreGB';e={[math]::Round($_.SizeRemaining/1GB,1)}}, @{n='TotalGB';e={[math]::Round($_.Size/1GB,1)}}`
- Linux: `df -h -x tmpfs -x devtmpfs`

## 2. Quién ocupa

Por tamaño, de mayor a menor, y **sin bajar a más de dos niveles**: un recorrido
completo de un disco de red tarda minutos y casi nunca hace falta.

- Windows: los directorios de primer nivel de la unidad afectada, sumando tamaños
- Linux: `du -xh --max-depth=2 / 2>/dev/null | sort -rh | head -20`

El `-x` de `du` importa: sin él se cuentan montajes de red y el resultado no
describe ese disco.

## 3. Los sospechosos de siempre

Mira, en este orden, que es el de más espacio por menos riesgo:

1. Temporales de Windows (`C:\Windows\Temp`, `%TEMP%`) o `/var/tmp`
2. Logs rotados viejos — `/var/log/*.gz`, `C:\inetpub\logs`
3. Caché de actualizaciones: `C:\Windows\SoftwareDistribution\Download`, `/var/cache`
4. Volcados de memoria y ficheros `.dmp`
5. Perfiles de usuario que ya no se usan

## 4. La propuesta

Di **cuánto se libera con cada cosa** antes de proponer nada. «Borrar temporales»
sin una cifra al lado no permite decidir si merece la pena parar a hacerlo.

Y avisa de lo que no se debe tocar aunque ocupe: `pagefile.sys`, `hiberfil.sys` y
las instantáneas de volumen tienen dueño y borrarlas tiene consecuencias que no se
ven hasta después.
