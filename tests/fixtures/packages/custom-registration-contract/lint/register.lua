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
    target = "variable=message#/resolve/default",
    handler = "check",
  })

  lint:rule({
    id = "policy/declared",
    title = "Declared custom registration rule",
    help = "Use this rule for registration contract checks.",
    target = "enum=tier",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
