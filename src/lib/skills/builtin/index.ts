// ── Built-in Skills Index — registers all bundled SysAdmin skills ────────────

import { registerSkill } from '$lib/skills/skill-engine';
import type { SkillDef } from '$lib/skills/skill-engine';

// ── 1. DNS TROUBLESHOOT ─────────────────────────────────────────────────────

const dnsTroubleshoot: SkillDef = {
    id: 'dns-troubleshoot',
    name: 'Diagnóstico DNS',
    nameEN: 'DNS Troubleshooting',
    description: 'Diagnostica problemas de resolución DNS: verifica servidores, consultas, cache y configuración.',
    descriptionEN: 'Diagnose DNS resolution issues: check servers, queries, cache and configuration.',
    icon: '🌐',
    category: 'network',
    os: 'both',
    tags: ['dns', 'network', 'resolve', 'nslookup', 'dig', 'domain'],
    version: '1.0',
    phases: [
        {
            id: 'dns-check-config',
            name: 'Verificar configuración DNS',
            nameEN: 'Check DNS config',
            prompt: `Check the current DNS configuration on this host.
On Linux: cat /etc/resolv.conf and check systemd-resolved status if available.
On Windows: Get-DnsClientServerAddress and Get-DnsClientCache | Select -First 10.
Report what DNS servers are configured and if they look correct.`,
        },
        {
            id: 'dns-test-resolution',
            name: 'Probar resolución',
            nameEN: 'Test resolution',
            prompt: `Test DNS resolution for common domains and the domain the user mentioned (if any).
On Linux: use dig or nslookup to resolve google.com and any user-specified domain.
On Windows: use Resolve-DnsName for the same.
Compare results from the configured DNS server vs a public DNS (8.8.8.8).
Report any differences or failures.`,
        },
        {
            id: 'dns-check-latency',
            name: 'Medir latencia DNS',
            nameEN: 'Measure DNS latency',
            prompt: `Measure DNS query latency to the configured servers.
On Linux: dig @<dns-server> google.com and check "Query time" in the output.
On Windows: Measure-Command { Resolve-DnsName google.com } | Select TotalMilliseconds.
Report the latency and whether it's acceptable (< 100ms = good, > 500ms = problem).`,
            optional: true,
        },
        {
            id: 'dns-summary',
            name: 'Resumen y recomendaciones',
            nameEN: 'Summary and recommendations',
            prompt: `Based on all previous phases, provide a summary:
1. Is DNS working correctly?
2. What issues were found (if any)?
3. Recommended fixes (flush cache, change DNS server, restart service, etc.)
Do NOT execute any commands. Just analyze and recommend.`,
            expectVerdict: true,
        },
    ],
};

// ── 2. SSL CERTIFICATE CHECK ────────────────────────────────────────────────

const sslCheck: SkillDef = {
    id: 'ssl-check',
    name: 'Verificación SSL/TLS',
    nameEN: 'SSL/TLS Certificate Check',
    description: 'Verifica certificados SSL: expiración, cadena de confianza, configuración TLS.',
    descriptionEN: 'Verify SSL certificates: expiration, trust chain, TLS configuration.',
    icon: '🔒',
    category: 'security',
    os: 'both',
    tags: ['ssl', 'tls', 'certificate', 'https', 'expiry', 'openssl'],
    version: '1.0',
    phases: [
        {
            id: 'ssl-list-certs',
            name: 'Listar certificados',
            nameEN: 'List certificates',
            prompt: `List SSL certificates on this host.
On Linux: find /etc/ssl/certs /etc/pki/tls/certs -name "*.pem" -o -name "*.crt" 2>/dev/null | head -20, and check if certbot/letsencrypt is installed.
On Windows: Get-ChildItem Cert:\\LocalMachine\\My | Select Subject, NotAfter, Thumbprint.
Report what certificates exist and their locations.`,
        },
        {
            id: 'ssl-check-expiry',
            name: 'Verificar expiración',
            nameEN: 'Check expiration',
            prompt: `Check certificate expiration dates.
If the user mentioned a domain: use openssl s_client -connect <domain>:443 to fetch the cert, then openssl x509 -noout -dates.
On Windows: Check Cert:\\LocalMachine\\My for certs expiring within 30 days.
Flag any certificates expiring within 30 days as WARNING and within 7 days as CRITICAL.`,
        },
        {
            id: 'ssl-test-tls',
            name: 'Probar conexión TLS',
            nameEN: 'Test TLS connection',
            prompt: `Test the TLS handshake and protocol version.
If a domain was specified: openssl s_client -connect <domain>:443 -brief (Linux) or Test-NetConnection <domain> -Port 443 (Windows).
Check which TLS version is negotiated (TLS 1.2/1.3 = good, TLS 1.0/1.1 = bad).
Report the cipher suite being used.`,
            optional: true,
        },
        {
            id: 'ssl-summary',
            name: 'Resumen y alertas',
            nameEN: 'Summary and alerts',
            prompt: `Summarize all SSL/TLS findings:
1. Certificates found and their status
2. Any expiring certificates (with exact dates)
3. TLS version compliance
4. Recommended actions (renew, upgrade TLS, etc.)
Do NOT execute commands.`,
            expectVerdict: true,
        },
    ],
};

// ── 3. DISK CLEANUP ─────────────────────────────────────────────────────────

const diskCleanup: SkillDef = {
    id: 'disk-cleanup',
    name: 'Limpieza de Disco',
    nameEN: 'Disk Cleanup Analysis',
    description: 'Analiza uso de disco, identifica archivos grandes, logs antiguos y directorios temporales.',
    descriptionEN: 'Analyze disk usage, identify large files, old logs and temp directories.',
    icon: '💾',
    category: 'storage',
    os: 'both',
    tags: ['disk', 'space', 'cleanup', 'storage', 'df', 'du', 'logs', 'temp'],
    version: '1.0',
    phases: [
        {
            id: 'disk-overview',
            name: 'Vista general del disco',
            nameEN: 'Disk overview',
            prompt: `Get a high-level view of disk usage.
On Linux: df -h to show filesystem usage, identify any partition above 80%.
On Windows: Get-PSDrive -PSProvider FileSystem | Select Name, @{N='Used(GB)';E={[math]::Round($_.Used/1GB,1)}}, @{N='Free(GB)';E={[math]::Round($_.Free/1GB,1)}}, @{N='%Used';E={[math]::Round($_.Used/($_.Used+$_.Free)*100,1)}}.
Report which drives/partitions are at risk.`,
        },
        {
            id: 'disk-large-files',
            name: 'Buscar archivos grandes',
            nameEN: 'Find large files',
            prompt: `Find the largest files consuming disk space.
On Linux: find / -xdev -type f -size +100M -exec ls -lh {} \\; 2>/dev/null | sort -k5 -h | tail -20.
On Windows: Get-ChildItem C:\\ -Recurse -File -ErrorAction SilentlyContinue | Where Length -gt 100MB | Sort Length -Descending | Select -First 20 FullName, @{N='Size(MB)';E={[math]::Round($_.Length/1MB,1)}}.
Report the top space consumers.`,
        },
        {
            id: 'disk-temp-logs',
            name: 'Analizar temporales y logs',
            nameEN: 'Analyze temps and logs',
            prompt: `Check temp directories and log files for cleanup opportunities.
On Linux: du -sh /tmp /var/tmp /var/log /var/cache 2>/dev/null and find /var/log -name "*.gz" -o -name "*.old" -o -name "*.[0-9]" 2>/dev/null | wc -l.
On Windows: Check size of $env:TEMP, C:\\Windows\\Temp, C:\\Windows\\Logs and check Windows Update cleanup opportunities.
Report total reclaimable space.`,
        },
        {
            id: 'disk-recommendations',
            name: 'Recomendaciones de limpieza',
            nameEN: 'Cleanup recommendations',
            prompt: `Based on findings, provide specific cleanup recommendations:
1. Safe-to-delete items (temp files, old logs, package cache)
2. Items that need review before deletion (large files, old backups)
3. Estimated space recoverable
4. A single cleanup command for safe items (if applicable, wrap in <EXECUTE>)
Mark the response with <VERDICT>DONE</VERDICT> if no critical issues, or <VERDICT>ESCALATE</VERDICT> if disk is critically full.`,
            expectVerdict: true,
        },
    ],
};

// ── 4. SERVICE HEALTH CHECK ─────────────────────────────────────────────────

const serviceHealth: SkillDef = {
    id: 'service-health',
    name: 'Salud de Servicios',
    nameEN: 'Service Health Check',
    description: 'Verifica estado de servicios críticos, puertos abiertos, consumo de recursos y logs de error.',
    descriptionEN: 'Check critical service status, open ports, resource usage and error logs.',
    icon: '⚙️',
    category: 'services',
    os: 'both',
    tags: ['service', 'health', 'systemctl', 'status', 'port', 'process', 'restart'],
    version: '1.0',
    phases: [
        {
            id: 'svc-list-running',
            name: 'Servicios activos',
            nameEN: 'Running services',
            prompt: `List currently running services and their status.
On Linux: systemctl list-units --type=service --state=running --no-pager | head -30.
On Windows: Get-Service | Where Status -eq Running | Sort DisplayName | Select -First 30 Name, DisplayName, Status.
Report the number of running services and highlight any critical ones (web servers, databases, ssh, etc.).`,
        },
        {
            id: 'svc-check-failed',
            name: 'Servicios fallidos',
            nameEN: 'Failed services',
            prompt: `Check for failed or inactive services.
On Linux: systemctl list-units --type=service --state=failed --no-pager and systemctl list-units --type=service --state=inactive --no-pager | grep -E "nginx|apache|mysql|postgres|ssh|docker|kubelet|redis".
On Windows: Get-Service | Where Status -eq Stopped | Where StartType -eq Automatic | Select Name, DisplayName.
Report any services that SHOULD be running but aren't.`,
        },
        {
            id: 'svc-check-ports',
            name: 'Verificar puertos',
            nameEN: 'Check ports',
            prompt: `Check which ports are open and listening.
On Linux: ss -tulnp | head -25.
On Windows: Get-NetTCPConnection -State Listen | Select LocalPort, OwningProcess | Sort LocalPort.
Cross-reference with expected services. Flag any unexpected ports or missing expected ports.`,
        },
        {
            id: 'svc-resource-usage',
            name: 'Uso de recursos',
            nameEN: 'Resource usage',
            prompt: `Check resource usage of top processes.
On Linux: ps aux --sort=-%cpu | head -15 and free -h.
On Windows: Get-Process | Sort CPU -Desc | Select -First 15 Name, CPU, @{N='Mem(MB)';E={[math]::Round($_.WorkingSet/1MB,1)}}.
Flag any process using more than 80% CPU or services with memory leaks (growing RSS).`,
        },
        {
            id: 'svc-summary',
            name: 'Resumen de salud',
            nameEN: 'Health summary',
            prompt: `Provide a health report card:
- Overall status: HEALTHY / WARNING / CRITICAL
- Failed services that need attention
- Port anomalies
- Resource bottlenecks
- Recommended actions
Do NOT execute commands. Use <VERDICT>DONE</VERDICT> if healthy, <VERDICT>ESCALATE</VERDICT> if critical issues found.`,
            expectVerdict: true,
        },
    ],
};

// ── 5. FIREWALL AUDIT ───────────────────────────────────────────────────────

const firewallAudit: SkillDef = {
    id: 'firewall-audit',
    name: 'Auditoría de Firewall',
    nameEN: 'Firewall Audit',
    description: 'Revisa reglas de firewall, puertos expuestos, políticas por defecto y recomendaciones de hardening.',
    descriptionEN: 'Review firewall rules, exposed ports, default policies and hardening recommendations.',
    icon: '🔥',
    category: 'security',
    os: 'both',
    tags: ['firewall', 'iptables', 'ufw', 'nftables', 'netfilter', 'windows-firewall', 'ports', 'security'],
    version: '1.0',
    phases: [
        {
            id: 'fw-check-status',
            name: 'Estado del firewall',
            nameEN: 'Firewall status',
            prompt: `Check if the firewall is active and what tool is managing it.
On Linux: Check ufw status, iptables -L -n --line-numbers, firewall-cmd --state, nft list ruleset (try all, report which is active).
On Windows: Get-NetFirewallProfile | Select Name, Enabled, DefaultInboundAction, DefaultOutboundAction.
Report: Is the firewall enabled? What is the default policy (allow/deny)?`,
        },
        {
            id: 'fw-list-rules',
            name: 'Listar reglas',
            nameEN: 'List rules',
            prompt: `List all firewall rules.
On Linux: Use the active firewall tool found in the previous phase to list all rules (input, output, forward).
On Windows: Get-NetFirewallRule -Enabled True -Direction Inbound | Select -First 30 DisplayName, Action, @{N='Port';E={(Get-NetFirewallPortFilter -AssociatedNetFirewallRule $_).LocalPort}}.
Report: How many rules exist? Any overly permissive rules (allow all from any)?`,
        },
        {
            id: 'fw-exposed-ports',
            name: 'Puertos expuestos',
            nameEN: 'Exposed ports',
            prompt: `Cross-reference firewall rules with actual listening ports.
On Linux: ss -tulnp to get listening ports, compare with firewall allow rules. Flag ports that are listening but NOT explicitly allowed, or allowed but NOT listening.
On Windows: Compare Get-NetTCPConnection -State Listen with firewall inbound allow rules.
Report any mismatches.`,
        },
        {
            id: 'fw-recommendations',
            name: 'Recomendaciones de hardening',
            nameEN: 'Hardening recommendations',
            prompt: `Provide firewall hardening recommendations:
1. Default policy should be DENY for inbound
2. Remove any "allow all" rules
3. Restrict SSH/RDP to specific source IPs
4. Enable logging for denied connections
5. Close unused open ports
Rate the current firewall posture: STRONG / MODERATE / WEAK.
Do NOT execute commands. Use <VERDICT>DONE</VERDICT>.`,
            expectVerdict: true,
        },
    ],
};

// ── REGISTER ALL ────────────────────────────────────────────────────────────

export function registerBuiltinSkills() {
    registerSkill(dnsTroubleshoot);
    registerSkill(sslCheck);
    registerSkill(diskCleanup);
    registerSkill(serviceHealth);
    registerSkill(firewallAudit);
}
