function register(lint)
  lint:on({
    stage = "parse",
    entity = "workspace",
    rule = {
          id = "payments/check",
          title = "Payments check",
          help = "Fix the payments policy.",
        },
    handler = "check",
  })
end

function check(ctx)
  return {}
end
