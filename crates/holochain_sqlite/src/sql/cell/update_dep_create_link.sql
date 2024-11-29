UPDATE
  DhtOp
SET
  when_integrated = :when_integrated,
  validation_stage = NULL
WHERE
  validation_stage = 3
  AND validation_status IS NOT NULL
  AND DhtOp.type = :delete_link
  AND EXISTS(
    SELECT
      1
    FROM
      DhtOp AS DhtOpDep
    WHERE
      DhtOpDep.action_hash = DhtOp.dependency
      AND DhtOpDep.when_integrated IS NOT NULL
      AND DhtOpDep.type = :create_link
    LIMIT
      1
  )
RETURNING hash