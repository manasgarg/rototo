function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
      id = "policy/conflict",
      title = "Policy conflict A",
      help = "Policy conflict A.",
    },
    handler = "check",
  })

  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
      id = "policy/conflict",
      title = "Policy conflict B",
      help = "Policy conflict B.",
    },
    handler = "check",
  })
end

function check(ctx)
  return {}
end
