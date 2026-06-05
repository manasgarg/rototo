function register(lint)
  lint:on({
    stage = "parse",
    entity = "value",
    rule = {
          id = "policy/declared",
          title = "Declared custom registration rule",
          help = "Use this rule for registration contract checks.",
        },
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "predicate",
    rule = {
          id = "policy/declared",
          title = "Declared custom registration rule",
          help = "Use this rule for registration contract checks.",
        },
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "value",
    field = "value.",
    rule = {
          id = "policy/declared",
          title = "Declared custom registration rule",
          help = "Use this rule for registration contract checks.",
        },
    handler = "check",
  })

  lint:on({
    stage = "value",
    entity = "value",
    field = "value.bad segment",
    rule = {
          id = "policy/declared",
          title = "Declared custom registration rule",
          help = "Use this rule for registration contract checks.",
        },
    handler = "check",
  })
end

function check(ctx)
  return {}
end
