-- Every committed history mutation changes a durable cursor revision.
CREATE TABLE polling_history_revision (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>=0)
);
INSERT INTO polling_history_revision(singleton, revision) VALUES(1, 0);
CREATE TRIGGER polling_history_insert AFTER INSERT ON attempts BEGIN
    UPDATE polling_history_revision SET revision=revision+1 WHERE singleton=1;
END;
CREATE TRIGGER polling_history_update AFTER UPDATE ON attempts BEGIN
    UPDATE polling_history_revision SET revision=revision+1 WHERE singleton=1;
END;
