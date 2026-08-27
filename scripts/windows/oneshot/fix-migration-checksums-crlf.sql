UPDATE refinery_schema_history SET checksum='4381443328546418760' WHERE version=1 AND name='init';
UPDATE refinery_schema_history SET checksum='17494823703579882297' WHERE version=2 AND name='handoffs';
UPDATE refinery_schema_history SET checksum='4288838958242284757' WHERE version=3 AND name='decay';
UPDATE refinery_schema_history SET checksum='10631560961098348868' WHERE version=4 AND name='embeddings';
UPDATE refinery_schema_history SET checksum='8918373404379914778' WHERE version=5 AND name='cascade_indexes';
UPDATE refinery_schema_history SET checksum='5562778923159616632' WHERE version=6 AND name='wiki_migrations';
UPDATE refinery_schema_history SET checksum='9069949130429052508' WHERE version=7 AND name='observations_fts_and_link_index';
UPDATE refinery_schema_history SET checksum='165526243542000286' WHERE version=8 AND name='performance_indexes';
UPDATE refinery_schema_history SET checksum='6637262488138575225' WHERE version=9 AND name='sessions_supported_agent_kinds';
UPDATE refinery_schema_history SET checksum='16797321673002355635' WHERE version=10 AND name='observation_extension_events';
UPDATE refinery_schema_history SET checksum='6219450647781566218' WHERE version=11 AND name='sessions_antigravity_agent_kind';
UPDATE refinery_schema_history SET checksum='13844742656979771713' WHERE version=12 AND name='fts_remove_diacritics';
UPDATE refinery_schema_history SET checksum='145923184916883550' WHERE version=13 AND name='cross_project_links';
UPDATE refinery_schema_history SET checksum='652269621890290127' WHERE version=14 AND name='users';
UPDATE refinery_schema_history SET checksum='4608119891662367894' WHERE version=15 AND name='pages_author_id';
UPDATE refinery_schema_history SET checksum='16872537024714094366' WHERE version=16 AND name='audit_log_author_id';
UPDATE refinery_schema_history SET checksum='8415781115942856553' WHERE version=17 AND name='pages_fts_index_path';
UPDATE refinery_schema_history SET checksum='15172411195625771508' WHERE version=18 AND name='enforce_project_workspace_pairing';
UPDATE refinery_schema_history SET checksum='14254357495931938100' WHERE version=19 AND name='repair_orphan_observation_attribution';
UPDATE refinery_schema_history SET checksum='14444437648300490471' WHERE version=20 AND name='sessions_grok_agent_kind';
UPDATE refinery_schema_history SET checksum='13159116313697887703' WHERE version=21 AND name='auto_improve_pending_proposals';
UPDATE refinery_schema_history SET checksum='4183647845445018781' WHERE version=22 AND name='auto_improve_scheduler';
UPDATE refinery_schema_history SET checksum='17192365329939385411' WHERE version=23 AND name='auto_improve_patch_proposals';
UPDATE refinery_schema_history SET checksum='6345391763602988538' WHERE version=24 AND name='auto_improve_rejections';
UPDATE refinery_schema_history SET checksum='18378232996491366138' WHERE version=25 AND name='sessions_pi_agent_kind';
UPDATE refinery_schema_history SET checksum='4647651842484653297' WHERE version=26 AND name='sessions_zero_agent_kind';
UPDATE refinery_schema_history SET checksum='16995010226076692832' WHERE version=27 AND name='repair_nongit_fragment_attribution';
-- Gerado em 2026-08-27.
--
-- Por que: refinery::embed_migrations! embute o conteudo dos .sql em tempo de
-- compilacao. O binario de 24/08 foi compilado com o working tree em CRLF, e
-- gravou no banco os checksums desse conteudo. Depois de normalizar o repo para
-- LF, o binario novo calcula checksums diferentes para as MESMAS migrations e o
-- refinery recusa a subir com "applied migration V1__init is different than
-- filesystem one".
--
-- Seguro porque: o conteudo semantico das V01..V27 nao mudou (diff ignorando CR
-- e vazio) e os nomes das 27 batem um a um com os do binario novo. Só o
-- checksum registrado esta desatualizado.
--
-- Validado numa copia do banco antes de ser proposto: as 23 migrations novas
-- (V28..V50) aplicaram, e 5035 paginas / 294003 observacoes / 1774 sessoes
-- sobreviveram.
