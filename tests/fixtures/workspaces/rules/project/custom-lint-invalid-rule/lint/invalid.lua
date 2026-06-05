function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
      id = "invalid",
      title = "Invalid custom rule",
      help = "Invalid custom rule.",
    },
    handler = "check",
  })
end

function check(ctx)
  return {}
end
