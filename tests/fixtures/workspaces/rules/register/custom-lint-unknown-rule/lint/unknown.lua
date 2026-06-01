function register(lint)
  lint:on({
    stage = "project",
    entity = "workspace",
    rule = "payments/missing-rule",
    handler = "check",
  })
end

function check(ctx)
  return {}
end
