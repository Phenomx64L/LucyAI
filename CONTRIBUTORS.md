# Contributors & History

## Founder & Lead Architect
- **Iván Eduardo Luna (@Phenomx64L)** — Concepto, arquitectura fundacional, Sprints 1-3.

## Sprint Timeline & Key Milestones
- **Sprint 1 (Feb 2026)**: ReAct Self-Correction, Live Trace, and Engine Setup.
- **Sprint 2 (Mar 2026)**: Semantic Search with Ollama, NLP Local Integrations.
- **Sprint 3 (Apr 2026)**: Tiered Memory (Core, Working, Episodic), Graphify AST.

## Architect's Vision & Key Decisions
Lucy no es "un juguete de IA genérico", es un Sistema Operativo Agéntico para SysAdmins. Estas son las decisiones arquitectónicas clave adoptadas por Iván:

- **Elegir Rust + Tauri en lugar de Electron**: Seleccionado para garantizar seguridad de memoria, control granular de OS y extrema eficiencia en Windows/Linux.
- **SQLite Tiered en lugar de Cloud DB**: Control local absoluto sobre los datos, sin costos recurrentes ni filtración de secretos, manteniendo la privacidad de infraestructura.
- **Ollama Embeddings en lugar de OpenAI API**: Privacidad total para la memoria semántica, permitiendo auditorías de incidentes sin fugas de información.

## Contribution Guidelines
Las reglas internas del sistema, en especial el `System Prompt` en `ai.rs` y la estructura de memoria episódica, son el resultado directo de más de 10 años de experiencia real en administración de sistemas.

Se exige a todos los contribuyentes que respeten esta filosofía. Las *pull requests* que alteren el ruteo de seguridad o las reglas autónomas de SysAdmin deberán venir acompañadas de una alta justificación técnica alineada a la visión original.
