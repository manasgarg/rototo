function register(lint)
  lint:on({
    stage = "parse",
    entity = "workspace",
    rule = "payments/check",
    handler = "check",
  })
end

function check(ctx)
  return {}
end
