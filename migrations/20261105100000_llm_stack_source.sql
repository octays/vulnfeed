-- Which classifier produced the stacks: '' keyword pass, 'llm' model pass.
ALTER TABLE vuln_information ADD COLUMN stacks_source TEXT NOT NULL DEFAULT '';
ALTER TABLE security_notice ADD COLUMN stacks_source TEXT NOT NULL DEFAULT '';
