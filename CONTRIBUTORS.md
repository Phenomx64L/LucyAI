# Contributors & History

## Founder & Lead Architect
- **Iván Eduardo Luna (@Phenomx64L)** — Concepto, arquitectura fundacional, Sprints 1-3.

## Sprint Timeline & Key Milestones
- **Sprint 1 (Feb 2026)**: ReAct Self-Correction, Live Trace, and Engine Setup.
- **Sprint 2 (Mar 2026)**: Semantic Search with Ollama, NLP Local Integrations.
- **Sprint 3 (Apr 2026)**: Tiered Memory (Core, Working, Episodic), Graphify AST.
- **Ago 2026 — v2.0.1**: la interfaz en cinco idiomas, y el bake-off que decidió
  dejar el WebView (egui contra iced, medido en una sesión RDP sin GPU).
- **Sep 2026 — v2.1.0**: el shell nativo sustituye a la app Tauri. El instalador
  pasa de 213 MB a 6,6. Auditoría del núcleo contra capacidades de agente de
  frontera: 35 huecos, diecinueve cerrados.

## Architect's Vision & Key Decisions
Lucy no es "un juguete de IA genérico", es un Sistema Operativo Agéntico para SysAdmins. Estas son las decisiones arquitectónicas clave adoptadas por Iván:

- **Elegir Rust en lugar de Electron**: seguridad de memoria, control granular del
  sistema operativo y eficiencia en Windows. La primera encarnación fue Tauri 2 +
  SvelteKit, que cumplía lo del lenguaje pero seguía arrastrando un motor de
  navegador; en agosto de 2026 se sustituyó por una interfaz nativa en `egui`, y
  ahí la decisión original terminó de cumplirse — un solo ejecutable, sin WebView.
  La V1 sigue entera en la historia, bajo el tag `v1-svelte-final`.
- **SQLite Tiered en lugar de Cloud DB**: Control local absoluto sobre los datos, sin costos recurrentes ni filtración de secretos, manteniendo la privacidad de infraestructura.
- **Ollama Embeddings en lugar de OpenAI API**: Privacidad total para la memoria semántica, permitiendo auditorías de incidentes sin fugas de información.

## Contribution Guidelines
Las reglas internas del sistema, en especial el *System Prompt* —hoy en
`lucy-core/src/prompt.rs`, antes en `ai.rs` de la app Tauri— y la estructura de
memoria episódica, son el resultado directo de más de 10 años de experiencia real
en administración de sistemas.

Se exige a todos los contribuyentes que respeten esta filosofía. Las *pull requests* que alteren el ruteo de seguridad o las reglas autónomas de SysAdmin deberán venir acompañadas de una alta justificación técnica alineada a la visión original.
