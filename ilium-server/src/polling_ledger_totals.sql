-- Transactional projections: indefinite attempt history is never scanned on admission.
CREATE TABLE polling_totals (
    bucket TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL CHECK(typeof(attempts)='integer' AND attempts>=0),
    unknown_attempts INTEGER NOT NULL CHECK(typeof(unknown_attempts)='integer' AND unknown_attempts>=0),
    known_cost_nanodollars INTEGER NOT NULL CHECK(typeof(known_cost_nanodollars)='integer' AND known_cost_nanodollars>=0),
    reserved_nanodollars INTEGER NOT NULL CHECK(typeof(reserved_nanodollars)='integer' AND reserved_nanodollars>=0),
    known_input_tokens INTEGER NOT NULL CHECK(typeof(known_input_tokens)='integer' AND known_input_tokens>=0),
    unknown_days INTEGER NOT NULL CHECK(typeof(unknown_days)='integer' AND unknown_days>=0)
);
INSERT INTO polling_totals SELECT bucket, SUM(attempts), SUM(unknown_attempts), SUM(known_cost_nanodollars), SUM(reserved_nanodollars), SUM(known_input_tokens), SUM(unknown_days) FROM (
SELECT 'all' AS bucket, 1 AS attempts, (state='unknown') AS unknown_attempts, COALESCE(cost_nanodollars,0) AS known_cost_nanodollars, CASE WHEN state IN ('prepared','dispatched','unknown') THEN reserved_nanodollars ELSE 0 END AS reserved_nanodollars, COALESCE(input_tokens,0) AS known_input_tokens, (day IS NULL) AS unknown_days FROM attempts
UNION ALL
SELECT 'month:' || month AS bucket, 1 AS attempts, (state='unknown') AS unknown_attempts, COALESCE(cost_nanodollars,0) AS known_cost_nanodollars, CASE WHEN state IN ('prepared','dispatched','unknown') THEN reserved_nanodollars ELSE 0 END AS reserved_nanodollars, COALESCE(input_tokens,0) AS known_input_tokens, (day IS NULL) AS unknown_days FROM attempts
UNION ALL
SELECT 'day:' || day AS bucket, 1 AS attempts, (state='unknown') AS unknown_attempts, COALESCE(cost_nanodollars,0) AS known_cost_nanodollars, CASE WHEN state IN ('prepared','dispatched','unknown') THEN reserved_nanodollars ELSE 0 END AS reserved_nanodollars, COALESCE(input_tokens,0) AS known_input_tokens, (day IS NULL) AS unknown_days FROM attempts WHERE day IS NOT NULL
) GROUP BY bucket;

CREATE TRIGGER polling_totals_insert AFTER INSERT ON attempts BEGIN
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'all', 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE 1
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'month:' || NEW.month, 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE 1
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'day:' || NEW.day, 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE NEW.day IS NOT NULL
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
END;

CREATE TRIGGER polling_totals_update AFTER UPDATE ON attempts BEGIN
    UPDATE polling_totals SET attempts=attempts-(1), unknown_attempts=unknown_attempts-((OLD.state='unknown')), known_cost_nanodollars=known_cost_nanodollars-(COALESCE(OLD.cost_nanodollars,0)), reserved_nanodollars=reserved_nanodollars-(CASE WHEN OLD.state IN ('prepared','dispatched','unknown') THEN OLD.reserved_nanodollars ELSE 0 END), known_input_tokens=known_input_tokens-(COALESCE(OLD.input_tokens,0)), unknown_days=unknown_days-((OLD.day IS NULL)) WHERE bucket='all';
    UPDATE polling_totals SET attempts=attempts-(1), unknown_attempts=unknown_attempts-((OLD.state='unknown')), known_cost_nanodollars=known_cost_nanodollars-(COALESCE(OLD.cost_nanodollars,0)), reserved_nanodollars=reserved_nanodollars-(CASE WHEN OLD.state IN ('prepared','dispatched','unknown') THEN OLD.reserved_nanodollars ELSE 0 END), known_input_tokens=known_input_tokens-(COALESCE(OLD.input_tokens,0)), unknown_days=unknown_days-((OLD.day IS NULL)) WHERE bucket='month:' || OLD.month;
    UPDATE polling_totals SET attempts=attempts-(1), unknown_attempts=unknown_attempts-((OLD.state='unknown')), known_cost_nanodollars=known_cost_nanodollars-(COALESCE(OLD.cost_nanodollars,0)), reserved_nanodollars=reserved_nanodollars-(CASE WHEN OLD.state IN ('prepared','dispatched','unknown') THEN OLD.reserved_nanodollars ELSE 0 END), known_input_tokens=known_input_tokens-(COALESCE(OLD.input_tokens,0)), unknown_days=unknown_days-((OLD.day IS NULL)) WHERE bucket='day:' || OLD.day AND OLD.day IS NOT NULL;
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'all', 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE 1
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'month:' || NEW.month, 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE 1
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
    INSERT INTO polling_totals (bucket, attempts, unknown_attempts, known_cost_nanodollars, reserved_nanodollars, known_input_tokens, unknown_days)
    SELECT 'day:' || NEW.day, 1, (NEW.state='unknown'), COALESCE(NEW.cost_nanodollars,0), CASE WHEN NEW.state IN ('prepared','dispatched','unknown') THEN NEW.reserved_nanodollars ELSE 0 END, COALESCE(NEW.input_tokens,0), (NEW.day IS NULL) WHERE NEW.day IS NOT NULL
    ON CONFLICT(bucket) DO UPDATE SET attempts=attempts+excluded.attempts, unknown_attempts=unknown_attempts+excluded.unknown_attempts, known_cost_nanodollars=known_cost_nanodollars+excluded.known_cost_nanodollars, reserved_nanodollars=reserved_nanodollars+excluded.reserved_nanodollars, known_input_tokens=known_input_tokens+excluded.known_input_tokens, unknown_days=unknown_days+excluded.unknown_days;
END;
