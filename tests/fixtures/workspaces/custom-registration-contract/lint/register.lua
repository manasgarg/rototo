function register(lint)
  lint:on({
    stage = "parse",
    entity = "value",
    rule = "policy/declared",
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "predicate",
    rule = "policy/declared",
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "value",
    field = "value.",
    rule = "policy/declared",
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "value",
    field = "value.bad segment",
    rule = "policy/declared",
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "value",
    rule = "policy/missing",
    handler = "check",
  })
end

function check(ctx)
  return {}
end
