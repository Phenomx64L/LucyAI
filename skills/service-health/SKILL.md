---
name: service-health
description: Revisa servicios caídos o inestables, por qué fallaron y si arrancan al reintentarlo.
---

# Salud de servicios

## 1. Qué está caído que debería estar arriba

- Windows: `Get-Service | Where-Object { $_.StartType -eq 'Automatic' -and $_.Status -ne 'Running' }`
- Linux: `systemctl list-units --type=service --state=failed`

Un servicio manual parado **no es un hallazgo**. Solo los automáticos, que son los
que alguien decidió que arrancaran solos.

## 2. Por qué

Para cada uno, mira su historia antes de tocarlo:

- Windows: `Get-WinEvent -FilterHashtable @{LogName='System'; ID=7031,7034,7036} -MaxEvents 30`
- Linux: `journalctl -u <servicio> -n 50 --no-pager`

Lo que buscas es la diferencia entre «no arrancó nunca» y «arrancó y se murió»:
son dos problemas distintos y llevan a sitios distintos.

## 3. Dependencias

Un servicio caído que depende de otro caído no es dos incidentes, es uno. Antes de
proponer arrancar nada, comprueba de qué depende — arrancar el de abajo suele
resolver los tres de arriba.

## 4. La propuesta

Arrancar un servicio es un cambio: proponlo, no lo hagas. Y si el mismo servicio
lleva varios reinicios en el log, **dilo en vez de proponer otro**: reiniciar algo
que ya se ha reiniciado cinco veces esconde la causa en lugar de arreglarla.

## Al terminar

Nombra el servicio, la causa que hayas encontrado, y qué harías. Si no encontraste
la causa, dilo — «está caído y el log no dice por qué» es información útil y una
suposición no lo es.
