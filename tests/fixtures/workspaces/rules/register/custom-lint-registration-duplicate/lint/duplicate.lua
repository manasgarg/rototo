function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = "payments/noop",
    handler = "check_workspace",
  })
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = "payments/noop",
    handler = "check_workspace",
  })
end

function check_workspace(ctx)
  return {}
end
