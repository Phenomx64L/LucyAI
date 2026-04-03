// ── ReportGenerator.ts — PDF export for Lucy reports ─────────────────────────
// Uses jspdf + jspdf-autotable to generate professional SOC-style PDFs.

import jsPDF from 'jspdf';
import autoTable from 'jspdf-autotable';
import { saveFileDialog } from '$lib/lucy-api';

// ── COLORS ──────────────────────────────────────────────────────────────────
const SOC_BG: [number, number, number]     = [15, 23, 42];
const SOC_GREEN: [number, number, number]  = [0, 255, 149];
const SOC_RED: [number, number, number]    = [255, 70, 70];
const SOC_YELLOW: [number, number, number] = [255, 200, 50];
const WHITE: [number, number, number]      = [255, 255, 255];
const GRAY: [number, number, number]       = [148, 163, 184];

function initDoc(title: string, isEN: boolean): jsPDF {
    const doc = new jsPDF({ orientation: 'portrait', unit: 'mm', format: 'a4' });
    // Header bar
    doc.setFillColor(...SOC_BG);
    doc.rect(0, 0, 210, 28, 'F');
    doc.setFontSize(18);
    doc.setTextColor(...SOC_GREEN);
    doc.text('Lucy', 14, 16);
    doc.setFontSize(10);
    doc.setTextColor(...WHITE);
    doc.text(title, 36, 16);
    // Date
    doc.setFontSize(8);
    doc.setTextColor(...GRAY);
    const now = new Date().toLocaleString();
    doc.text(`${isEN ? 'Generated' : 'Generado'}: ${now}`, 14, 24);
    return doc;
}

function addFooter(doc: jsPDF, isEN: boolean) {
    const pages = doc.getNumberOfPages();
    for (let i = 1; i <= pages; i++) {
        doc.setPage(i);
        doc.setFontSize(7);
        doc.setTextColor(120, 120, 120);
        doc.text(
            `Lucy Report — ${isEN ? 'Page' : 'Pag.'} ${i}/${pages}`,
            105, 290, { align: 'center' }
        );
    }
}

async function savePdf(doc: jsPDF, fileName: string) {
    const arrayBuf = doc.output('arraybuffer');
    const bytes = new Uint8Array(arrayBuf);
    let binary = '';
    for (let i = 0; i < bytes.length; i++) binary += String.fromCharCode(bytes[i]);
    const b64 = btoa(binary);
    return saveFileDialog(fileName, b64, 'PDF', ['pdf']);
}

// ── COMPLIANCE REPORT ───────────────────────────────────────────────────────

export interface ComplianceExportData {
    hostName: string;
    scanDate: string;
    passRate: number;
    passCount: number;
    failCount: number;
    results: Array<{
        title: string;
        category: string;
        severity: string;
        passed: boolean;
        remediation?: string;
    }>;
}

export async function exportCompliancePdf(data: ComplianceExportData, isEN: boolean): Promise<string> {
    const doc = initDoc(isEN ? 'Compliance Report' : 'Reporte de Cumplimiento', isEN);

    // Summary
    let y = 36;
    doc.setFontSize(11);
    doc.setTextColor(30, 30, 30);
    doc.text(`Host: ${data.hostName}`, 14, y);
    doc.text(`${isEN ? 'Date' : 'Fecha'}: ${data.scanDate}`, 120, y);
    y += 8;

    // Score box
    const scoreColor = data.passRate >= 80 ? SOC_GREEN : data.passRate >= 50 ? SOC_YELLOW : SOC_RED;
    doc.setFillColor(...scoreColor);
    doc.roundedRect(14, y, 40, 16, 3, 3, 'F');
    doc.setFontSize(16);
    doc.setTextColor(0, 0, 0);
    doc.text(`${data.passRate}%`, 34, y + 11, { align: 'center' });

    doc.setFontSize(10);
    doc.setTextColor(60, 60, 60);
    doc.text(`${isEN ? 'Passed' : 'Pasaron'}: ${data.passCount}  |  ${isEN ? 'Failed' : 'Fallaron'}: ${data.failCount}`, 60, y + 10);
    y += 24;

    // Table
    autoTable(doc, {
        startY: y,
        head: [[
            isEN ? 'Check' : 'Verificacion',
            isEN ? 'Category' : 'Categoria',
            isEN ? 'Severity' : 'Severidad',
            isEN ? 'Status' : 'Estado',
        ]],
        body: data.results.map(r => [
            r.title,
            r.category,
            r.severity.toUpperCase(),
            r.passed ? (isEN ? 'PASS' : 'OK') : 'FAIL',
        ]),
        styles: { fontSize: 8, cellPadding: 2 },
        headStyles: { fillColor: SOC_BG, textColor: SOC_GREEN, fontStyle: 'bold' },
        bodyStyles: { textColor: [30, 30, 30] },
        alternateRowStyles: { fillColor: [240, 245, 255] },
        didParseCell(data: any) {
            if (data.column.index === 3 && data.section === 'body') {
                const val = data.cell.raw as string;
                if (val === 'FAIL') {
                    data.cell.styles.textColor = SOC_RED;
                    data.cell.styles.fontStyle = 'bold';
                } else {
                    data.cell.styles.textColor = [0, 160, 80];
                }
            }
        },
    });

    // Failed remediation section
    const failed = data.results.filter(r => !r.passed && r.remediation);
    if (failed.length > 0) {
        const lastY = (doc as any).lastAutoTable?.finalY ?? 180;
        let ry = lastY + 10;
        if (ry > 260) { doc.addPage(); ry = 20; }
        doc.setFontSize(12);
        doc.setTextColor(...SOC_RED);
        doc.text(isEN ? 'Remediation Recommendations' : 'Recomendaciones de Remediacion', 14, ry);
        ry += 6;
        doc.setFontSize(8);
        doc.setTextColor(50, 50, 50);
        for (const f of failed) {
            if (ry > 275) { doc.addPage(); ry = 20; }
            doc.setFont('helvetica', 'bold');
            doc.text(`- ${f.title}:`, 14, ry);
            doc.setFont('helvetica', 'normal');
            ry += 4;
            const lines = doc.splitTextToSize(f.remediation || '', 180);
            doc.text(lines, 18, ry);
            ry += lines.length * 3.5 + 3;
        }
    }

    addFooter(doc, isEN);
    const fileName = `lucy-compliance-${data.hostName}-${new Date().toISOString().split('T')[0]}.pdf`;
    return savePdf(doc, fileName);
}

// ── INVENTORY REPORT ────────────────────────────────────────────────────────

export interface InventoryExportData {
    hostName: string;
    scanDate: string;
    ports: Array<{ port: number; protocol?: string; process?: string; state?: string }>;
    services: Array<{ name: string; status?: string; description?: string }>;
    software: Array<{ name: string; version?: string }>;
    certs: Array<{ subject?: string; issuer?: string; expiry?: string }>;
    scheduled: Array<{ name: string; schedule?: string; status?: string }>;
}

export async function exportInventoryPdf(data: InventoryExportData, isEN: boolean): Promise<string> {
    const doc = initDoc(isEN ? 'Infrastructure Inventory' : 'Inventario de Infraestructura', isEN);

    let y = 36;
    doc.setFontSize(11);
    doc.setTextColor(30, 30, 30);
    doc.text(`Host: ${data.hostName}`, 14, y);
    doc.text(`${isEN ? 'Date' : 'Fecha'}: ${data.scanDate}`, 120, y);
    y += 8;

    // Summary cards
    const counts = [
        { label: isEN ? 'Ports' : 'Puertos', n: data.ports.length },
        { label: isEN ? 'Services' : 'Servicios', n: data.services.length },
        { label: 'Software', n: data.software.length },
        { label: isEN ? 'Certs' : 'Certificados', n: data.certs.length },
        { label: isEN ? 'Tasks' : 'Tareas', n: data.scheduled.length },
    ];
    doc.setFontSize(9);
    counts.forEach((c, i) => {
        const x = 14 + i * 37;
        doc.setFillColor(230, 240, 255);
        doc.roundedRect(x, y, 34, 12, 2, 2, 'F');
        doc.setTextColor(60, 60, 60);
        doc.text(`${c.label}: ${c.n}`, x + 17, y + 7, { align: 'center' });
    });
    y += 18;

    // Ports table
    if (data.ports.length) {
        doc.setFontSize(11);
        doc.setTextColor(...SOC_BG);
        doc.text(isEN ? 'Open Ports' : 'Puertos Abiertos', 14, y);
        y += 2;
        autoTable(doc, {
            startY: y,
            head: [['Port', isEN ? 'Protocol' : 'Protocolo', isEN ? 'Process' : 'Proceso', isEN ? 'State' : 'Estado']],
            body: data.ports.slice(0, 50).map(p => [String(p.port), p.protocol || 'tcp', p.process || '-', p.state || 'LISTEN']),
            styles: { fontSize: 7, cellPadding: 1.5 },
            headStyles: { fillColor: SOC_BG, textColor: SOC_GREEN },
        });
        y = ((doc as any).lastAutoTable?.finalY ?? (y + 12)) + 8;
    }

    // Services table
    if (data.services.length) {
        if (y > 240) { doc.addPage(); y = 20; }
        doc.setFontSize(11);
        doc.setTextColor(...SOC_BG);
        doc.text(isEN ? 'Services' : 'Servicios', 14, y);
        y += 2;
        autoTable(doc, {
            startY: y,
            head: [[isEN ? 'Name' : 'Nombre', isEN ? 'Status' : 'Estado', isEN ? 'Description' : 'Descripcion']],
            body: data.services.slice(0, 50).map(s => [s.name, s.status || '-', (s.description || '-').substring(0, 60)]),
            styles: { fontSize: 7, cellPadding: 1.5 },
            headStyles: { fillColor: SOC_BG, textColor: SOC_GREEN },
        });
        y = ((doc as any).lastAutoTable?.finalY ?? (y + 12)) + 8;
    }

    // Software table
    if (data.software.length) {
        if (y > 240) { doc.addPage(); y = 20; }
        doc.setFontSize(11);
        doc.setTextColor(...SOC_BG);
        doc.text('Software', 14, y);
        y += 2;
        autoTable(doc, {
            startY: y,
            head: [[isEN ? 'Name' : 'Nombre', isEN ? 'Version' : 'Version']],
            body: data.software.slice(0, 80).map(s => [s.name, s.version || '-']),
            styles: { fontSize: 7, cellPadding: 1.5 },
            headStyles: { fillColor: SOC_BG, textColor: SOC_GREEN },
        });
    }

    addFooter(doc, isEN);
    const fileName = `lucy-inventory-${data.hostName}-${new Date().toISOString().split('T')[0]}.pdf`;
    return savePdf(doc, fileName);
}

// ── AUDIT TRAIL REPORT ──────────────────────────────────────────────────────

export interface AuditExportData {
    entries: Array<{
        timestamp: string;
        hostName: string;
        command: string;
        source: string;
        exitCode: number | null;
        durationMs: number | null;
        outputPreview: string;
    }>;
}

export async function exportAuditPdf(data: AuditExportData, isEN: boolean): Promise<string> {
    const doc = initDoc(isEN ? 'Audit Trail Report' : 'Reporte de Auditoria', isEN);

    let y = 36;
    doc.setFontSize(10);
    doc.setTextColor(50, 50, 50);
    doc.text(`${isEN ? 'Total entries' : 'Total registros'}: ${data.entries.length}`, 14, y);
    const failed = data.entries.filter(e => e.exitCode !== null && e.exitCode !== 0).length;
    doc.text(`${isEN ? 'Failed' : 'Fallidos'}: ${failed}`, 80, y);
    const uniqueH = [...new Set(data.entries.map(e => e.hostName))].length;
    doc.text(`Hosts: ${uniqueH}`, 130, y);
    y += 8;

    autoTable(doc, {
        startY: y,
        head: [[
            isEN ? 'Time' : 'Hora',
            'Host',
            isEN ? 'Command' : 'Comando',
            isEN ? 'Source' : 'Fuente',
            isEN ? 'Exit' : 'Codigo',
            'ms',
        ]],
        body: data.entries.slice(0, 200).map(e => [
            e.timestamp ? new Date(e.timestamp).toLocaleString() : '-',
            e.hostName,
            (e.command || '').substring(0, 60),
            e.source,
            e.exitCode !== null ? String(e.exitCode) : '-',
            e.durationMs !== null ? String(e.durationMs) : '-',
        ]),
        styles: { fontSize: 7, cellPadding: 1.5 },
        headStyles: { fillColor: SOC_BG, textColor: SOC_GREEN, fontStyle: 'bold' },
        columnStyles: {
            2: { cellWidth: 60 },
        },
        didParseCell(data: any) {
            if (data.column.index === 4 && data.section === 'body') {
                const val = data.cell.raw as string;
                if (val !== '-' && val !== '0') {
                    data.cell.styles.textColor = SOC_RED;
                    data.cell.styles.fontStyle = 'bold';
                }
            }
        },
    });

    addFooter(doc, isEN);
    const fileName = `lucy-audit-${new Date().toISOString().split('T')[0]}.pdf`;
    return savePdf(doc, fileName);
}
