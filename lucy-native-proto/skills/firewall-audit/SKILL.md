---
name: firewall-audit
description: Audita las reglas de cortafuegos — qué está abierto, hacia dónde y si alguna regla sobra.
---

# Auditoría de cortafuegos

## 1. Si está encendido

- Windows: `Get-NetFirewallProfile | Select-Object Name, Enabled`
- Linux: `firewall-cmd --state` o `ufw status` o `iptables -L -n` según lo que haya

Un perfil apagado es el hallazgo más importante que puedes encontrar aquí, y va
primero en el informe aunque lo descubras al final.

## 2. Qué está abierto de entrada

Solo las reglas **activas** y de **entrada**. Las de salida en una máquina de
usuario son casi siempre ruido, y las deshabilitadas no afectan a nada.

- Windows: `Get-NetFirewallRule -Direction Inbound -Enabled True -Action Allow | Get-NetFirewallPortFilter`
- Linux: `firewall-cmd --list-all` o `ufw status numbered`

## 3. Lo que merece una ceja

Marca, en este orden de gravedad:

1. Reglas abiertas a **cualquier origen** en puertos administrativos: 3389 (RDP),
   22 (SSH), 5985/5986 (WinRM), 445 (SMB)
2. Reglas que permiten **cualquier programa** en un puerto concreto
3. Reglas sin descripción ni fecha, que nadie sabe ya por qué están
4. Puertos abiertos sin nada escuchando detrás — sobran

## 4. Contrastar con la realidad

Una regla abierta para un servicio que ya no existe es una regla que sobra. Cruza
lo abierto con lo que de verdad escucha:

- Windows: `Get-NetTCPConnection -State Listen | Select-Object LocalPort, OwningProcess`
- Linux: `ss -tlnp`

## Al terminar

**No propongas cerrar nada sin decir qué se rompería.** Cerrar un puerto es la
clase de acción que se descubre mal a las nueve de la mañana del lunes. Di qué
regla sobra, por qué crees que sobra, y qué comprobarías antes de tocarla.
