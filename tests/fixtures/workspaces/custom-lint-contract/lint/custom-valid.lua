function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = "payments/max-token-budget",
    handler = "reject_variable",
  })
end

function reject_variable(ctx)
  if ctx.target.id == "custom-valid" then
    return {
      {
        message = "custom lint rejected " .. ctx.target.id
      }
    }
  end
  return {}
end
