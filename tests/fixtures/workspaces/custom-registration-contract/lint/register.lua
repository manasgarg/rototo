function register(lint)
  lint:rule({
    id = "policy/declared",
    title = "Declared custom registration rule",
    help = "Use this rule for registration contract checks.",
    target = "variables",
    handler = "check",
  })

  lint:rule({
    id = "policy/declared",
    title = "Declared custom registration rule",
    help = "Use this rule for registration contract checks.",
    target = "/unknown",
    handler = "check",
  })

  lint:rule({
    id = "policy/declared",
    title = "Declared custom registration rule",
    help = "Use this rule for registration contract checks.",
    target = "/variables/message/value",
    handler = "check",
  })

  lint:rule({
    id = "policy/declared",
    title = "Declared custom registration rule",
    help = "Use this rule for registration contract checks.",
    target = "/variables/message/rules/not-number",
    handler = "check",
  })
end

function check(workspace, target)
  return {}
end
