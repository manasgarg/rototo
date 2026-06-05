function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = {
          id = "fixture/custom-variable-rejected",
          title = "Custom variable lint rejected the variable",
          help = "Change the fixture or the Lua lint rule.",
        },
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
