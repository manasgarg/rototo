function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = "fixture/custom-variable-rejected",
    handler = "reject_variable",
  })
end

function reject_variable(ctx)
  if ctx.target.id == "custom-lint" then
    return {
      {
        message = "custom lint rejected " .. ctx.target.id
      }
    }
  end
  return {}
end
