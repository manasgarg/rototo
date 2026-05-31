function register(lint)
  lint:on({
    stage = "value",
    entity = "value",
    field = "value.max_output_tokens",
    rule = "payments/max-token-budget",
    handler = "check_token_budget",
  })

  lint:on({
    stage = "parse",
    entity = "value",
    rule = "payments/max-token-budget",
    handler = "check_token_budget",
  })

  lint:on({
    stage = "value",
    entity = "value",
    rule = "payments/missing-rule",
    handler = "check_token_budget",
  })

  lint:on({
    stage = "value",
    entity = "value",
    rule = "payments/max-token-budget",
    handler = "missing_handler",
  })
end

function check_token_budget(ctx)
  local budget = ctx.target.value.max_output_tokens
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
