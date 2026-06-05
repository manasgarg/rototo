function register(lint)
  lint:on({
    stage = "policy",
    entity = "variable",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
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
