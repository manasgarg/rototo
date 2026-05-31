function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = "payments/max-token-budget",
    handler = "fail_variable",
  })
end

function fail_variable(ctx)
  if ctx.target.id == "custom-failed" then
    error("script failed for " .. ctx.target.id)
  end
  return {}
end
