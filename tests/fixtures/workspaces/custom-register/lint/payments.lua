function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "check_token_budget",
  })

  lint:on({
    stage = "parse",
    entity = "value",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "check_token_budget",
  })

  lint:on({
    stage = "value",
    entity = "value",
    rule = {
          id = "payments/max-token-budget",
          title = "Token budget exceeds payments policy",
          help = "Lower max_output_tokens or update the payments policy.",
        },
    handler = "missing_handler",
  })
end

function check_token_budget(ctx)
  local budget = ctx.target.value[1]
  if budget ~= nil and budget > 5000 then
    return {
      {
        message = ctx.target.variable.id .. "." .. ctx.target.name
          .. " exceeds 5000 output tokens"
      }
    }
  end
  return {}
end
