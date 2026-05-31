function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    rule = "fixture/custom-value-rejected",
    handler = "reject_value",
  })
end

function reject_value(ctx)
  if ctx.target.variable.id == "custom-value-lint" then
    return {
      {
        message = "custom value lint rejected "
          .. ctx.target.variable.id .. "." .. ctx.target.name
      }
    }
  end
  return {}
end
